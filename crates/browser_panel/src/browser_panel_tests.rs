use super::*;
use browser::test_support::{FakeBrowser, fake_connection};
use gpui::{TestAppContext, UpdateGlobal as _, VisualTestContext};
use serde_json::{Value, json};
use workspace::AppState;

fn panel_with_fake_chrome(
    cx: &mut TestAppContext,
) -> (Entity<BrowserPanel>, FakeBrowser, &mut VisualTestContext) {
    let app_state = cx.update(|cx| {
        let state = AppState::test(cx);
        editor::init(cx);
        init(cx);
        state
    });
    let (connection, chrome) = fake_connection(cx);
    let browser = cx.new(|_| Browser::new(Rc::new(move |_| Task::ready(Ok(connection.clone())))));
    let (panel, cx) = cx.add_window_view(|window, cx| {
        BrowserPanel::with_browser(
            browser,
            app_state.fs.clone(),
            WeakEntity::new_invalid(),
            window,
            cx,
        )
    });
    (panel, chrome, cx)
}

async fn expect(chrome: &FakeBrowser, method: &str, result: Value) -> Value {
    let command = chrome.next_command().await;
    assert_eq!(command["method"], method, "unexpected command {command}");
    chrome.reply(&command, result).await;
    command
}

async fn answer_new_tab(chrome: &FakeBrowser) {
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
    // The drawn page area sized the browser, so the tab is sized before it is shown.
    expect(chrome, "Emulation.setDeviceMetricsOverride", json!({})).await;
}

#[gpui::test]
async fn test_typing_an_address_opens_it_in_a_new_tab(cx: &mut TestAppContext) {
    let (panel, chrome, cx) = panel_with_fake_chrome(cx);

    // With no tab, focusing the panel lands in the address bar.
    panel.update_in(cx, |panel, window, cx| {
        window.focus(&panel.focus_handle(cx), cx);
        assert!(panel.address_bar.focus_handle(cx).is_focused(window));
    });
    cx.simulate_input("localhost:5173");
    cx.dispatch_action(menu::Confirm);

    answer_new_tab(&chrome).await;
    let navigate = expect(&chrome, "Page.navigate", json!({ "frameId": "tab-1" })).await;
    assert_eq!(navigate["params"]["url"], "http://localhost:5173");
    cx.run_until_parked();

    panel.update_in(cx, |panel, window, cx| {
        assert!(
            panel.page_focus_handle.is_focused(window),
            "the page takes focus once the address opens"
        );
        assert_eq!(panel.browser.read(cx).tabs().len(), 1);
        assert!(panel.error.is_none());
    });
}

#[gpui::test]
async fn test_failed_navigation_shows_its_reason(cx: &mut TestAppContext) {
    let (panel, chrome, cx) = panel_with_fake_chrome(cx);
    panel.update_in(cx, |panel, window, cx| {
        window.focus(&panel.focus_handle(cx), cx)
    });
    cx.simulate_input("nowhere.invalid");
    cx.dispatch_action(menu::Confirm);
    answer_new_tab(&chrome).await;
    expect(
        &chrome,
        "Page.navigate",
        json!({ "errorText": "net::ERR_NAME_NOT_RESOLVED" }),
    )
    .await;
    cx.run_until_parked();

    panel.read_with(cx, |panel, _| {
        assert_eq!(
            panel.error.as_deref(),
            Some("could not open https://nowhere.invalid: net::ERR_NAME_NOT_RESOLVED")
        );
    });
}

#[gpui::test]
async fn test_keys_typed_on_the_page_reach_chrome(cx: &mut TestAppContext) {
    let (panel, chrome, cx) = panel_with_fake_chrome(cx);
    let opened = panel.update(cx, |panel, cx| {
        panel
            .browser
            .update(cx, |browser, cx| browser.new_tab(None, cx))
    });
    answer_new_tab(&chrome).await;
    opened.await.unwrap();
    panel.update_in(cx, |panel, window, cx| {
        window.focus(&panel.page_focus_handle, cx);
    });

    cx.simulate_keystrokes("a");
    let typed = expect(&chrome, "Input.insertText", json!({})).await;
    assert_eq!(typed["params"]["text"], "a");

    cx.simulate_keystrokes("enter");
    let enter = expect(&chrome, "Input.dispatchKeyEvent", json!({})).await;
    assert_eq!(enter["params"]["key"], "Enter");
    expect(&chrome, "Input.dispatchKeyEvent", json!({})).await;

    cx.write_to_clipboard(ClipboardItem::new_string("pasted text".to_string()));
    cx.simulate_keystrokes("cmd-v");
    let pasted = expect(&chrome, "Input.insertText", json!({})).await;
    assert_eq!(pasted["params"]["text"], "pasted text");
}

#[gpui::test]
async fn test_agents_get_the_browser_tools_while_the_setting_allows(cx: &mut TestAppContext) {
    let (panel, _chrome, cx) = panel_with_fake_chrome(cx);
    let fs = project::FakeFs::new(cx.executor());
    fs.insert_tree("/project", json!({ "index.html": "" }))
        .await;
    let project = project::Project::test(fs, [std::path::Path::new("/project")], cx).await;
    let server_ids = |cx: &mut VisualTestContext| {
        project.read_with(cx, |project, cx| {
            project
                .context_server_store()
                .read(cx)
                .configured_server_ids()
        })
    };

    panel.update(cx, |panel, cx| {
        panel.serve_tools_to_agents(project.clone(), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        server_ids(cx),
        vec![context_server::ContextServerId(
            AGENT_TOOLS_SERVER_ID.into()
        )]
    );
    let url = panel.read_with(cx, |panel, _| {
        panel.agent_tools_server.as_ref().unwrap().url().to_string()
    });
    let settings = project.read_with(cx, |project, cx| {
        project
            .context_server_store()
            .read(cx)
            .settings_for_server(&context_server::ContextServerId(
                AGENT_TOOLS_SERVER_ID.into(),
            ))
            .cloned()
    });
    match settings {
        Some(ContextServerSettings::Http {
            url: registered,
            headers,
            ..
        }) => {
            assert_eq!(registered, url);
            assert!(headers["Authorization"].starts_with("Bearer "));
        }
        _ => panic!("expected the browser tools as an HTTP server"),
    }

    cx.update(|_, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.browser.get_or_insert_default().agent_tools = Some(false);
            });
        });
    });
    cx.run_until_parked();
    assert!(server_ids(cx).is_empty());
    panel.read_with(cx, |panel, _| assert!(panel.agent_tools_server.is_none()));
}
