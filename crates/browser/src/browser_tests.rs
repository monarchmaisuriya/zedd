use super::*;
use crate::cdp::test_support::{FakeBrowser, fake_connection};
use gpui::{Entity, TestAppContext};
use std::cell::Cell;

/// A browser whose connector hands out `connection` and counts how often it was asked.
fn browser_with(
    connection: Arc<CdpConnection>,
    cx: &mut TestAppContext,
) -> (Entity<Browser>, Rc<Cell<usize>>) {
    let connects = Rc::new(Cell::new(0));
    let browser = cx.new(|_| {
        Browser::new(Rc::new({
            let connects = connects.clone();
            move |_| {
                connects.set(connects.get() + 1);
                Task::ready(Ok(connection.clone()))
            }
        }))
    });
    (browser, connects)
}

/// Answers the next command, which must be `method`, with `result`, and returns the command.
async fn expect(chrome: &FakeBrowser, method: &str, result: Value) -> Value {
    let command = chrome.next_command().await;
    assert_eq!(command["method"], method, "unexpected command {command}");
    chrome.reply(&command, result).await;
    command
}

/// Opens a tab with target "tab-1" and session "session-1", answering Chrome's side.
async fn open_tab(
    browser: &Entity<Browser>,
    chrome: &FakeBrowser,
    url: Option<&str>,
    cx: &mut TestAppContext,
) -> Task<Result<()>> {
    let opened = browser.update(cx, |browser, cx| {
        browser.new_tab(url.map(str::to_string), cx)
    });
    expect(chrome, "Target.setDiscoverTargets", json!({})).await;
    expect(
        chrome,
        "Target.createTarget",
        json!({ "targetId": "tab-1" }),
    )
    .await;
    expect(
        chrome,
        "Target.attachToTarget",
        json!({ "sessionId": "session-1" }),
    )
    .await;
    expect(chrome, "Page.enable", json!({})).await;
    expect(chrome, "Runtime.enable", json!({})).await;
    opened
}

#[gpui::test]
async fn test_new_tab_attaches_and_loads_the_url(cx: &mut TestAppContext) {
    let (connection, chrome) = fake_connection(cx);
    let (browser, _) = browser_with(connection, cx);

    let opened = open_tab(&browser, &chrome, Some("localhost:3000"), cx).await;
    let navigate = expect(&chrome, "Page.navigate", json!({ "frameId": "tab-1" })).await;
    assert_eq!(navigate["sessionId"], "session-1");
    assert_eq!(navigate["params"]["url"], "http://localhost:3000");
    opened.await.unwrap();

    chrome
        .emit(
            None,
            "Target.targetInfoChanged",
            json!({ "targetInfo": { "targetId": "tab-1", "type": "page", "url": "http://localhost:3000/", "title": "Home" } }),
        )
        .await;
    cx.run_until_parked();
    browser.read_with(cx, |browser, _| {
        let tab = browser.active_tab().unwrap();
        assert_eq!(tab.url, "http://localhost:3000/");
        assert_eq!(tab.title, "Home");
    });
}

#[gpui::test]
async fn test_failed_navigation_reports_chromes_reason(cx: &mut TestAppContext) {
    let (connection, chrome) = fake_connection(cx);
    let (browser, _) = browser_with(connection, cx);
    open_tab(&browser, &chrome, None, cx).await.await.unwrap();

    let navigate = browser.update(cx, |browser, cx| browser.navigate("nope.invalid", cx));
    expect(
        &chrome,
        "Page.navigate",
        json!({ "frameId": "tab-1", "errorText": "net::ERR_NAME_NOT_RESOLVED" }),
    )
    .await;
    assert_eq!(
        navigate.await.unwrap_err().to_string(),
        "could not open https://nope.invalid: net::ERR_NAME_NOT_RESOLVED"
    );
}

#[gpui::test]
async fn test_shown_tab_streams_frames_at_the_viewport_size(cx: &mut TestAppContext) {
    let (connection, chrome) = fake_connection(cx);
    let (browser, _) = browser_with(connection, cx);
    browser.update(cx, |browser, cx| {
        browser.set_viewport(
            Viewport {
                width: 400.,
                height: 300.,
                scale_factor: 2.,
            },
            cx,
        );
        browser.set_visible(true, cx);
    });

    let opened = browser.update(cx, |browser, cx| browser.new_tab(None, cx));
    for method in [
        "Target.setDiscoverTargets",
        "Target.createTarget",
        "Target.attachToTarget",
        "Page.enable",
        "Runtime.enable",
    ] {
        let result = match method {
            "Target.createTarget" => json!({ "targetId": "tab-1" }),
            "Target.attachToTarget" => json!({ "sessionId": "session-1" }),
            _ => json!({}),
        };
        expect(&chrome, method, result).await;
    }
    let metrics = expect(&chrome, "Emulation.setDeviceMetricsOverride", json!({})).await;
    assert_eq!(metrics["params"]["width"], 400);
    assert_eq!(metrics["params"]["deviceScaleFactor"], 2.0);
    let screencast = expect(&chrome, "Page.startScreencast", json!({})).await;
    assert_eq!(screencast["params"]["maxWidth"], 800);
    assert_eq!(screencast["params"]["maxHeight"], 600);
    opened.await.unwrap();

    let mut jpeg = Vec::new();
    image::RgbImage::from_pixel(4, 2, image::Rgb([255, 0, 0]))
        .write_to(
            &mut std::io::Cursor::new(&mut jpeg),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
    chrome
        .emit(
            Some("session-1"),
            "Page.screencastFrame",
            json!({
                "data": base64::engine::general_purpose::STANDARD.encode(&jpeg),
                "metadata": {},
                "sessionId": 7,
            }),
        )
        .await;
    let ack = expect(&chrome, "Page.screencastFrameAck", json!({})).await;
    assert_eq!(ack["params"]["sessionId"], 7);
    browser.read_with(cx, |browser, _| {
        let frame = browser.active_tab().unwrap().frame.clone().unwrap();
        let pixel = &frame.as_bytes(0).unwrap()[0..4];
        assert!(
            pixel[2] > 200 && pixel[0] < 50,
            "red arrives as BGRA, got {pixel:?}"
        );
    });

    browser.update(cx, |browser, cx| browser.set_visible(false, cx));
    expect(&chrome, "Page.stopScreencast", json!({})).await;
}

#[gpui::test]
async fn test_page_events_update_console_dialog_and_loading(cx: &mut TestAppContext) {
    let (connection, chrome) = fake_connection(cx);
    let (browser, _) = browser_with(connection, cx);
    open_tab(&browser, &chrome, None, cx).await.await.unwrap();

    chrome
        .emit(
            Some("session-1"),
            "Runtime.consoleAPICalled",
            json!({ "type": "warning", "args": [
                { "type": "string", "value": "count" },
                { "type": "number", "value": 3 },
                { "type": "object", "description": "Object" },
            ] }),
        )
        .await;
    chrome
        .emit(
            Some("session-1"),
            "Runtime.exceptionThrown",
            json!({ "exceptionDetails": { "text": "Uncaught", "exception": { "description": "TypeError: x is undefined" } } }),
        )
        .await;
    chrome
        .emit(
            Some("session-1"),
            "Page.javascriptDialogOpening",
            json!({ "type": "confirm", "message": "Delete?", "defaultPrompt": "" }),
        )
        .await;
    chrome
        .emit(
            Some("session-1"),
            "Page.frameStartedLoading",
            json!({ "frameId": "tab-1" }),
        )
        .await;
    cx.run_until_parked();

    let loaded = browser.update(cx, |browser, cx| {
        assert_eq!(
            browser.console_messages(),
            vec![
                ConsoleMessage {
                    level: "warning".into(),
                    text: "count 3 Object".to_string()
                },
                ConsoleMessage {
                    level: "error".into(),
                    text: "TypeError: x is undefined".to_string()
                },
            ]
        );
        let tab = browser.active_tab().unwrap();
        assert_eq!(tab.dialog.as_ref().unwrap().message, "Delete?");
        assert!(tab.loading);
        browser.wait_for_load(Duration::from_secs(60), cx)
    });

    let answered = browser.update(cx, |browser, cx| browser.handle_dialog(true, None, cx));
    let handle = expect(&chrome, "Page.handleJavaScriptDialog", json!({})).await;
    assert_eq!(handle["params"]["accept"], true);
    answered.await.unwrap();
    chrome
        .emit(
            Some("session-1"),
            "Page.javascriptDialogClosed",
            json!({ "result": true }),
        )
        .await;
    chrome
        .emit(
            Some("session-1"),
            "Page.frameStoppedLoading",
            json!({ "frameId": "tab-1" }),
        )
        .await;
    loaded.await;
    browser.read_with(cx, |browser, _| {
        let tab = browser.active_tab().unwrap();
        assert!(tab.dialog.is_none());
        assert!(!tab.loading);
    });
}

#[gpui::test]
async fn test_keys_carry_codes_text_and_editing_commands(cx: &mut TestAppContext) {
    let (connection, chrome) = fake_connection(cx);
    let (browser, _) = browser_with(connection, cx);
    open_tab(&browser, &chrome, None, cx).await.await.unwrap();

    let pressed = browser.update(cx, |browser, cx| {
        browser.press_key("Enter", KeyModifiers::default(), cx)
    });
    let down = expect(&chrome, "Input.dispatchKeyEvent", json!({})).await;
    let up = expect(&chrome, "Input.dispatchKeyEvent", json!({})).await;
    pressed.await.unwrap();
    assert_eq!(down["params"]["type"], "keyDown");
    assert_eq!(down["params"]["text"], "\r");
    assert_eq!(down["params"]["windowsVirtualKeyCode"], 13);
    assert_eq!(up["params"]["type"], "keyUp");

    let select_all = browser.update(cx, |browser, cx| {
        browser.press_key(
            "a",
            KeyModifiers {
                meta: true,
                ..KeyModifiers::default()
            },
            cx,
        )
    });
    let down = expect(&chrome, "Input.dispatchKeyEvent", json!({})).await;
    let up = expect(&chrome, "Input.dispatchKeyEvent", json!({})).await;
    select_all.await.unwrap();
    assert_eq!(down["params"]["type"], "rawKeyDown");
    assert_eq!(down["params"]["commands"], json!(["selectAll"]));
    assert_eq!(down["params"]["modifiers"], 4);
    assert!(down["params"].get("text").is_none());
    assert!(up["params"].get("commands").is_none());

    let unknown = browser.update(cx, |browser, cx| {
        browser.press_key("Hyper", KeyModifiers::default(), cx)
    });
    assert_eq!(
        unknown.await.unwrap_err().to_string(),
        "unknown key `Hyper`"
    );
}

#[gpui::test]
async fn test_popups_from_a_tab_become_tabs_and_closed_targets_leave(cx: &mut TestAppContext) {
    let (connection, chrome) = fake_connection(cx);
    let (browser, _) = browser_with(connection, cx);
    open_tab(&browser, &chrome, None, cx).await.await.unwrap();

    chrome
        .emit(
            None,
            "Target.targetCreated",
            json!({ "targetInfo": { "targetId": "unrelated", "type": "page", "openerId": "elsewhere" } }),
        )
        .await;
    chrome
        .emit(
            None,
            "Target.targetCreated",
            json!({ "targetInfo": { "targetId": "popup", "type": "page", "openerId": "tab-1" } }),
        )
        .await;
    let attach = expect(
        &chrome,
        "Target.attachToTarget",
        json!({ "sessionId": "session-2" }),
    )
    .await;
    assert_eq!(attach["params"]["targetId"], "popup");
    expect(&chrome, "Page.enable", json!({})).await;
    expect(&chrome, "Runtime.enable", json!({})).await;
    cx.run_until_parked();
    browser.read_with(cx, |browser, _| {
        assert_eq!(browser.tabs().len(), 2);
        assert_eq!(browser.active_tab().unwrap().target_id, "popup");
    });

    chrome
        .emit(
            None,
            "Target.targetDestroyed",
            json!({ "targetId": "popup" }),
        )
        .await;
    cx.run_until_parked();
    browser.read_with(cx, |browser, _| {
        assert_eq!(browser.tabs().len(), 1);
        assert_eq!(browser.active_tab().unwrap().target_id, "tab-1");
    });
}

#[gpui::test]
async fn test_browser_reconnects_after_chrome_exits(cx: &mut TestAppContext) {
    let (connection, chrome) = fake_connection(cx);
    let (browser, connects) = browser_with(connection, cx);
    open_tab(&browser, &chrome, None, cx).await.await.unwrap();
    assert_eq!(connects.get(), 1);

    drop(chrome);
    cx.run_until_parked();
    let reopened = browser.update(cx, |browser, cx| browser.new_tab(None, cx));
    cx.run_until_parked();
    browser.read_with(cx, |browser, _| assert!(browser.tabs().is_empty()));
    assert_eq!(
        connects.get(),
        2,
        "a dead connection is replaced, not reused"
    );
    assert!(
        reopened.await.is_err(),
        "the fake hands out the same dead connection"
    );
}

#[test]
fn test_urls_get_the_scheme_an_address_bar_would_assume() {
    assert_eq!(normalize_url("localhost:3000/a"), "http://localhost:3000/a");
    assert_eq!(normalize_url("127.0.0.1:8080"), "http://127.0.0.1:8080");
    assert_eq!(normalize_url("app.localhost"), "http://app.localhost");
    assert_eq!(normalize_url("example.com"), "https://example.com");
    assert_eq!(normalize_url(" https://zed.dev "), "https://zed.dev");
    assert_eq!(normalize_url("about:blank"), "about:blank");
}

#[gpui::test]
async fn test_picking_an_element_describes_it(cx: &mut TestAppContext) {
    let (connection, chrome) = fake_connection(cx);
    let (browser, _) = browser_with(connection, cx);
    open_tab(&browser, &chrome, None, cx).await.await.unwrap();
    let picked = Rc::new(Cell::new(None));
    let _subscription = cx.update(|cx| {
        let picked = picked.clone();
        cx.subscribe(&browser, move |_, event, _| {
            if let BrowserEvent::ElementPicked { backend_node_id } = event {
                picked.set(Some(*backend_node_id));
            }
        })
    });

    let picking = browser.update(cx, |browser, cx| browser.set_picking_element(true, cx));
    expect(&chrome, "DOM.enable", json!({})).await;
    expect(&chrome, "Overlay.enable", json!({})).await;
    let inspect = expect(&chrome, "Overlay.setInspectMode", json!({})).await;
    assert_eq!(inspect["params"]["mode"], "searchForNode");
    picking.await.unwrap();

    chrome
        .emit(
            Some("session-1"),
            "Overlay.inspectNodeRequested",
            json!({ "backendNodeId": 42 }),
        )
        .await;
    let stop = expect(&chrome, "Overlay.setInspectMode", json!({})).await;
    assert_eq!(stop["params"]["mode"], "none");
    assert_eq!(picked.get(), Some(42));
    browser.read_with(cx, |browser, _| assert!(!browser.is_picking_element()));

    let description = browser.update(cx, |browser, cx| browser.describe_element(42, cx));
    expect(
        &chrome,
        "DOM.resolveNode",
        json!({ "object": { "objectId": "node-42" } }),
    )
    .await;
    let describe = expect(
        &chrome,
        "Runtime.callFunctionOn",
        json!({ "result": { "value": {
            "url": "http://localhost:3000/",
            "markup": "<button class=\"cta\">Buy</button>",
            "selector": "main > button.cta",
        } } }),
    )
    .await;
    assert_eq!(describe["params"]["objectId"], "node-42");
    assert_eq!(
        description.await.unwrap(),
        "Element picked in the browser at http://localhost:3000/:\n```html\n<button class=\"cta\">Buy</button>\n```\nSelector: `main > button.cta`\n"
    );
}

/// Drives the installed Chrome: `cargo test -p browser -- --ignored`.
#[gpui::test]
#[ignore]
async fn test_real_chrome_reports_and_describes_the_picked_element(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let profile = tempfile::tempdir().unwrap();
    let chrome =
        crate::chrome::launch(&find_chrome(None).unwrap(), profile.path(), &cx.executor()).unwrap();
    let connection = chrome.connection.clone();
    let (browser, _) = browser_with(connection, cx);
    let picked = Rc::new(Cell::new(None));
    let _subscription = cx.update(|cx| {
        let picked = picked.clone();
        cx.subscribe(&browser, move |_, event, _| {
            if let BrowserEvent::ElementPicked { backend_node_id } = event {
                picked.set(Some(*backend_node_id));
            }
        })
    });
    let page = "data:text/html,<main id=app><button class=cta style=\"position:absolute;left:50px;top:50px;width:100px;height:40px\">Buy now</button></main>";
    browser
        .update(cx, |browser, cx| {
            browser.new_tab(Some(page.to_string()), cx)
        })
        .await
        .unwrap();
    cx.executor().timer(Duration::from_millis(500)).await;

    browser
        .update(cx, |browser, cx| browser.set_picking_element(true, cx))
        .await
        .unwrap();
    for action in [
        MouseAction::Moved,
        MouseAction::Pressed,
        MouseAction::Released,
    ] {
        let button = if action == MouseAction::Moved {
            MouseButton::None
        } else {
            MouseButton::Left
        };
        browser
            .update(cx, |browser, cx| {
                browser.dispatch_mouse(action, 100., 70., button, 1, KeyModifiers::default(), cx)
            })
            .await
            .unwrap();
    }
    for _ in 0..30 {
        if picked.get().is_some() {
            break;
        }
        cx.executor().timer(Duration::from_millis(100)).await;
    }
    let backend_node_id = picked.get().expect("Chrome reported no picked element");
    let description = browser
        .update(cx, |browser, cx| {
            browser.describe_element(backend_node_id, cx)
        })
        .await
        .unwrap();
    assert!(
        description.contains("<button class=\"cta\""),
        "{description}"
    );
    assert!(description.contains("Buy now</button>"), "{description}");
    assert!(
        description.contains("Selector: `main#app > button.cta`")
            || description.contains("Selector: `main#app"),
        "{description}"
    );
    drop(chrome);
}
