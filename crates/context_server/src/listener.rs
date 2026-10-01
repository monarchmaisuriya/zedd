//! An MCP server hosted inside zedd, reachable by agents over HTTP on the loopback interface.

use ::serde::{Deserialize, Serialize};
use anyhow::{Context as _, Result};
use collections::HashMap;
use futures::channel::oneshot;
use gpui::{App, AppContext as _, AsyncApp, Task};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
use std::{any::TypeId, cell::RefCell, rc::Rc, sync::Arc};
use util::ResultExt as _;

use crate::{
    client::{CspResult, RequestId, Response},
    types::{
        self, CallToolParams, CallToolResponse, Implementation, InitializeResponse,
        ListToolsResponse, ProtocolVersion, Request, ServerCapabilities, Tool, ToolAnnotations,
        ToolResponseContent, ToolsCapabilities,
        requests::{CallTool, Initialize, ListTools, Ping},
    },
};

/// An MCP server using the streamable HTTP transport with JSON replies.
///
/// It listens on 127.0.0.1 only and answers only requests that carry its bearer token and no
/// `Origin` header, so neither other local users of the port nor web pages can call its tools.
/// Dropping it stops the server.
pub struct McpServer {
    url: String,
    bearer_token: String,
    tools: Rc<RefCell<HashMap<&'static str, RegisteredTool>>>,
    handlers: Rc<RefCell<HashMap<&'static str, RequestHandler>>>,
    http_server: Arc<tiny_http::Server>,
    _dispatch_task: Task<()>,
}

struct RegisteredTool {
    tool: Tool,
    handler: ToolHandler,
}

type ToolHandler = Box<
    dyn Fn(
        Option<serde_json::Value>,
        &mut AsyncApp,
    ) -> Task<Result<ToolResponse<serde_json::Value>>>,
>;
type RequestHandler = Box<dyn Fn(RequestId, Option<Box<RawValue>>, &App) -> Task<String>>;

/// One JSON-RPC message received over HTTP, and where to send the reply. `None` means the
/// message was a notification, which gets no reply body.
struct IncomingMessage {
    body: String,
    reply_tx: oneshot::Sender<Option<String>>,
}

impl McpServer {
    /// Starts a server that introduces itself to clients as `server_name`.
    pub fn new(server_name: impl Into<String>, cx: &mut App) -> Result<Self> {
        let http_server = Arc::new(
            tiny_http::Server::http("127.0.0.1:0")
                .map_err(|error| anyhow::anyhow!("binding the MCP server: {error}"))?,
        );
        let port = http_server
            .server_addr()
            .to_ip()
            .context("MCP server is not on an IP address")?
            .port();
        let bearer_token: String = rand::random::<[u8; 32]>()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();

        let (request_tx, request_rx) = async_channel::unbounded::<IncomingMessage>();
        std::thread::Builder::new()
            .name("mcp-http".into())
            .spawn({
                let http_server = http_server.clone();
                let bearer_token = bearer_token.clone();
                move || {
                    // Each request gets its own thread, so a slow tool call does not hold up others.
                    for request in http_server.incoming_requests() {
                        let request_tx = request_tx.clone();
                        let bearer_token = bearer_token.clone();
                        std::thread::spawn(move || {
                            answer_http_request(request, &bearer_token, &request_tx)
                        });
                    }
                }
            })
            .context("starting the MCP server thread")?;

        let tools = Rc::new(RefCell::new(HashMap::default()));
        let handlers = Rc::new(RefCell::new(HashMap::default()));
        let server_info = Rc::new(server_name.into());
        let dispatch_task = cx.spawn({
            let tools = tools.clone();
            let handlers = handlers.clone();
            async move |cx| {
                while let Ok(message) = request_rx.recv().await {
                    match Self::dispatch(&message.body, &tools, &handlers, &server_info, cx) {
                        None => {
                            message.reply_tx.send(None).ok();
                        }
                        Some(reply) => cx
                            .spawn(async move |_| {
                                message.reply_tx.send(Some(reply.await)).ok();
                            })
                            .detach(),
                    }
                }
            }
        });

        Ok(Self {
            url: format!("http://127.0.0.1:{port}/mcp"),
            bearer_token,
            tools,
            handlers,
            http_server,
            _dispatch_task: dispatch_task,
        })
    }

    /// The URL clients post JSON-RPC messages to.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The value clients must send in the `Authorization` header.
    pub fn authorization_header(&self) -> String {
        format!("Bearer {}", self.bearer_token)
    }

    pub fn add_tool<T: McpServerTool + Clone + 'static>(&mut self, tool: T) {
        let mut settings = schemars::generate::SchemaSettings::draft07();
        settings.inline_subschemas = true;
        let mut generator = settings.into_generator();

        let input_schema = generator.root_schema_for::<T::Input>();

        let description = input_schema
            .get("description")
            .and_then(|desc| desc.as_str())
            .map(|desc| desc.to_string());
        debug_assert!(
            description.is_some(),
            "Input schema struct must include a doc comment for the tool description"
        );

        let registered_tool = RegisteredTool {
            tool: Tool {
                name: T::NAME.into(),
                title: None,
                description,
                input_schema: input_schema.into(),
                output_schema: if TypeId::of::<T::Output>() == TypeId::of::<()>() {
                    None
                } else {
                    Some(generator.root_schema_for::<T::Output>().into())
                },
                annotations: Some(tool.annotations()),
            },
            handler: Box::new({
                move |input_value, cx| {
                    let input = match input_value {
                        Some(input) => serde_json::from_value(input),
                        None => serde_json::from_value(serde_json::Value::Null),
                    };

                    let tool = tool.clone();
                    match input {
                        Ok(input) => cx.spawn(async move |cx| {
                            let output = tool.run(input, cx).await?;

                            Ok(ToolResponse {
                                content: output.content,
                                structured_content: serde_json::to_value(output.structured_content)
                                    .unwrap_or_default(),
                            })
                        }),
                        Err(err) => Task::ready(Err(err.into())),
                    }
                }
            }),
        };

        self.tools.borrow_mut().insert(T::NAME, registered_tool);
    }

    pub fn handle_request<R: Request>(
        &mut self,
        f: impl Fn(R::Params, &App) -> Task<Result<R::Response>> + 'static,
    ) {
        let f = Box::new(f);
        self.handlers.borrow_mut().insert(
            R::METHOD,
            Box::new(move |req_id, opt_params, cx| {
                let result = match opt_params {
                    Some(params) => serde_json::from_str(params.get()),
                    None => serde_json::from_value(serde_json::Value::Null),
                };

                let params: R::Params = match result {
                    Ok(params) => params,
                    Err(e) => {
                        return Task::ready(error_json(req_id, -32700, e.to_string()));
                    }
                };
                let task = f(params, cx);
                cx.background_spawn(async move {
                    match task.await {
                        Ok(result) => result_json(req_id, result),
                        Err(e) => error_json(req_id, -32603, e.to_string()),
                    }
                })
            }),
        );
    }

    /// Answers one JSON-RPC message, or returns `None` for a notification.
    fn dispatch(
        body: &str,
        tools: &Rc<RefCell<HashMap<&'static str, RegisteredTool>>>,
        handlers: &Rc<RefCell<HashMap<&'static str, RequestHandler>>>,
        server_name: &str,
        cx: &mut AsyncApp,
    ) -> Option<Task<String>> {
        let request: RawRequest = match serde_json::from_str(body) {
            Ok(request) => request,
            Err(error) => {
                log::error!("failed to parse incoming MCP message: {error}. Raw: {body}");
                return Some(Task::ready(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": { "code": -32700, "message": format!("Failed to parse: {error}") },
                    })
                    .to_string(),
                ));
            }
        };
        let request_id = request.id?;

        let reply = if request.method == Initialize::METHOD {
            result_json(request_id, initialize_response(request.params, server_name))
        } else if request.method == Ping::METHOD {
            result_json(request_id, serde_json::json!({}))
        } else if request.method == ListTools::METHOD {
            result_json(
                request_id,
                ListToolsResponse {
                    tools: tools.borrow().values().map(|t| t.tool.clone()).collect(),
                    next_cursor: None,
                    meta: None,
                },
            )
        } else if request.method == CallTool::METHOD {
            return Some(Self::call_tool(request_id, request.params, tools, cx));
        } else if let Some(handler) = handlers.borrow().get(&request.method.as_ref()) {
            return Some(cx.update(|cx| handler(request_id, request.params, cx)));
        } else {
            error_json(
                request_id,
                -32601,
                format!("unhandled method {}", request.method),
            )
        };
        Some(Task::ready(reply))
    }

    fn call_tool(
        request_id: RequestId,
        params: Option<Box<RawValue>>,
        tools: &Rc<RefCell<HashMap<&'static str, RegisteredTool>>>,
        cx: &mut AsyncApp,
    ) -> Task<String> {
        let params: Result<CallToolParams, _> = match params.as_ref() {
            Some(params) => serde_json::from_str(params.get()),
            None => serde_json::from_value(serde_json::Value::Null),
        };
        let params = match params {
            Ok(params) if tools.borrow().contains_key(params.name.as_str()) => params,
            Ok(params) => {
                let message = format!("Tool not found: {}", params.name);
                return Task::ready(error_json(request_id, -32602, message));
            }
            Err(error) => return Task::ready(error_json(request_id, -32602, error.to_string())),
        };

        let task = (tools.borrow()[params.name.as_str()].handler)(params.arguments, cx);
        cx.background_spawn(async move {
            let response = match task.await {
                Ok(result) => CallToolResponse {
                    content: result.content,
                    is_error: Some(false),
                    meta: None,
                    structured_content: if result.structured_content.is_null() {
                        None
                    } else {
                        Some(result.structured_content)
                    },
                },
                Err(err) => CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: err.to_string(),
                    }],
                    is_error: Some(true),
                    meta: None,
                    structured_content: None,
                },
            };
            result_json(request_id, response)
        })
    }
}

impl Drop for McpServer {
    fn drop(&mut self) {
        self.http_server.unblock();
    }
}

/// Speaks the client's protocol version when zedd knows it, else zedd's latest.
fn initialize_response(params: Option<Box<RawValue>>, server_name: &str) -> InitializeResponse {
    let requested_version = params
        .and_then(|params| {
            serde_json::from_str::<serde_json::Value>(params.get())
                .ok()?
                .get("protocolVersion")?
                .as_str()
                .map(str::to_string)
        })
        .filter(|version| {
            [
                types::VERSION_2024_11_05,
                types::VERSION_2025_03_26,
                types::VERSION_2025_06_18,
                types::LATEST_PROTOCOL_VERSION,
            ]
            .contains(&version.as_str())
        });
    InitializeResponse {
        protocol_version: ProtocolVersion(
            requested_version.unwrap_or_else(|| types::LATEST_PROTOCOL_VERSION.to_string()),
        ),
        capabilities: ServerCapabilities {
            tools: Some(ToolsCapabilities { list_changed: None }),
            ..ServerCapabilities::default()
        },
        server_info: Implementation {
            name: server_name.to_string(),
            title: None,
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: None,
        },
        meta: None,
    }
}

fn result_json<T: Serialize>(id: RequestId, result: T) -> String {
    serde_json::to_string(&Response {
        jsonrpc: "2.0",
        id,
        value: CspResult::Ok(Some(result)),
    })
    .unwrap_or_default()
}

fn error_json(id: RequestId, code: i32, message: String) -> String {
    serde_json::to_string(&Response::<()> {
        jsonrpc: "2.0",
        id,
        value: CspResult::Error(Some(crate::client::Error { message, code })),
    })
    .unwrap_or_default()
}

struct HttpReply {
    status: u16,
    body: String,
    is_json: bool,
}

impl HttpReply {
    fn refused(status: u16, reason: &str) -> Self {
        Self {
            status,
            body: reason.to_string(),
            is_json: false,
        }
    }
}

fn answer_http_request(
    mut request: tiny_http::Request,
    bearer_token: &str,
    request_tx: &async_channel::Sender<IncomingMessage>,
) {
    let reply = http_reply(&mut request, bearer_token, request_tx);
    let mut response = tiny_http::Response::from_string(reply.body).with_status_code(reply.status);
    let content_type = if reply.is_json {
        "application/json"
    } else {
        "text/plain"
    };
    if let Ok(header) = tiny_http::Header::from_bytes("Content-Type", content_type) {
        response.add_header(header);
    }
    request.respond(response).log_err();
}

fn http_reply(
    request: &mut tiny_http::Request,
    bearer_token: &str,
    request_tx: &async_channel::Sender<IncomingMessage>,
) -> HttpReply {
    let header = |name: &'static str| {
        request
            .headers()
            .iter()
            .find(|header| header.field.equiv(name))
            .map(|header| header.value.as_str().to_string())
    };
    // Browsers attach an Origin header; agents do not. Refusing it keeps web pages, including
    // ones open in zedd's own browser, from calling these tools.
    if header("Origin").is_some() {
        return HttpReply::refused(403, "requests from web pages are not accepted");
    }
    if header("Authorization").as_deref() != Some(&format!("Bearer {bearer_token}")) {
        return HttpReply::refused(401, "missing or wrong bearer token");
    }
    if *request.method() != tiny_http::Method::Post {
        return HttpReply::refused(405, "only POST is supported");
    }
    let mut body = String::new();
    if let Err(error) = request.as_reader().read_to_string(&mut body) {
        return HttpReply::refused(400, &format!("could not read the request body: {error}"));
    }

    let (reply_tx, reply_rx) = oneshot::channel();
    if request_tx
        .send_blocking(IncomingMessage { body, reply_tx })
        .is_err()
    {
        return HttpReply::refused(503, "the MCP server is shutting down");
    }
    match futures::executor::block_on(reply_rx) {
        Ok(Some(body)) => HttpReply {
            status: 200,
            body,
            is_json: true,
        },
        Ok(None) => HttpReply::refused(202, ""),
        Err(_) => HttpReply::refused(503, "the MCP server is shutting down"),
    }
}

pub trait McpServerTool {
    type Input: DeserializeOwned + JsonSchema;
    type Output: Serialize + JsonSchema;

    const NAME: &'static str;

    fn annotations(&self) -> ToolAnnotations {
        ToolAnnotations {
            title: None,
            read_only_hint: None,
            destructive_hint: None,
            idempotent_hint: None,
            open_world_hint: None,
        }
    }

    fn run(
        &self,
        input: Self::Input,
        cx: &mut AsyncApp,
    ) -> impl Future<Output = Result<ToolResponse<Self::Output>>>;
}

#[derive(Debug)]
pub struct ToolResponse<T> {
    pub content: Vec<ToolResponseContent>,
    pub structured_content: T,
}

#[derive(Debug, Serialize, Deserialize)]
struct RawRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<RequestId>,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Box<serde_json::value::RawValue>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::io::{Read as _, Write as _};

    /// Echoes its input back as text.
    #[derive(Clone, Deserialize, JsonSchema)]
    struct EchoInput {
        text: String,
    }

    #[derive(Clone)]
    struct EchoTool;

    impl McpServerTool for EchoTool {
        type Input = EchoInput;
        type Output = ();
        const NAME: &'static str = "echo";

        async fn run(&self, input: EchoInput, _: &mut AsyncApp) -> Result<ToolResponse<()>> {
            Ok(ToolResponse {
                content: vec![ToolResponseContent::Text { text: input.text }],
                structured_content: (),
            })
        }
    }

    struct HttpResult {
        status: u16,
        body: String,
    }

    /// Posts `body` from another thread, as an agent process would.
    async fn post(url: &str, headers: &[(&str, String)], body: serde_json::Value) -> HttpResult {
        let address = url
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap()
            .to_string();
        let mut request = format!(
            "POST /mcp HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nConnection: close\r\n"
        );
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        let body = body.to_string();
        request.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));

        let (tx, rx) = oneshot::channel();
        std::thread::spawn(move || {
            let mut stream = std::net::TcpStream::connect(address).unwrap();
            stream.write_all(request.as_bytes()).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            tx.send(response).ok();
        });
        let response = rx.await.unwrap();
        let (head, body) = response.split_once("\r\n\r\n").unwrap();
        HttpResult {
            status: head.split(' ').nth(1).unwrap().parse().unwrap(),
            body: body.to_string(),
        }
    }

    fn start_server(cx: &mut TestAppContext) -> McpServer {
        cx.executor().allow_parking();
        let mut server = cx.update(|cx| McpServer::new("test-server", cx).unwrap());
        server.add_tool(EchoTool);
        server
    }

    fn authorized(server: &McpServer) -> Vec<(&'static str, String)> {
        vec![("Authorization", server.authorization_header())]
    }

    #[gpui::test]
    async fn test_agents_can_initialize_list_and_call_tools_over_http(cx: &mut TestAppContext) {
        let server = start_server(cx);
        let headers = authorized(&server);

        let initialize = post(
            server.url(),
            &headers,
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": { "name": "agent", "version": "1" },
                },
            }),
        )
        .await;
        assert_eq!(initialize.status, 200);
        let initialize: serde_json::Value = serde_json::from_str(&initialize.body).unwrap();
        assert_eq!(initialize["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(initialize["result"]["serverInfo"]["name"], "test-server");
        assert!(initialize["result"]["capabilities"]["tools"].is_object());

        let initialized = post(
            server.url(),
            &headers,
            serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        )
        .await;
        assert_eq!(initialized.status, 202, "notifications get no reply body");

        let list = post(
            server.url(),
            &headers,
            serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        )
        .await;
        let list: serde_json::Value = serde_json::from_str(&list.body).unwrap();
        assert_eq!(list["result"]["tools"][0]["name"], "echo");
        assert_eq!(
            list["result"]["tools"][0]["description"],
            "Echoes its input back as text."
        );

        let call = post(
            server.url(),
            &headers,
            serde_json::json!({
                "jsonrpc": "2.0", "id": "three", "method": "tools/call",
                "params": { "name": "echo", "arguments": { "text": "hello" } },
            }),
        )
        .await;
        let call: serde_json::Value = serde_json::from_str(&call.body).unwrap();
        assert_eq!(call["id"], "three");
        assert_eq!(call["result"]["content"][0]["text"], "hello");
        assert_eq!(call["result"]["isError"], false);

        let unknown = post(
            server.url(),
            &headers,
            serde_json::json!({
                "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                "params": { "name": "missing" },
            }),
        )
        .await;
        let unknown: serde_json::Value = serde_json::from_str(&unknown.body).unwrap();
        assert_eq!(unknown["error"]["message"], "Tool not found: missing");
    }

    #[gpui::test]
    async fn test_requests_without_the_token_or_from_web_pages_are_refused(
        cx: &mut TestAppContext,
    ) {
        let server = start_server(cx);
        let list = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });

        let no_token = post(server.url(), &[], list.clone()).await;
        assert_eq!(no_token.status, 401);

        let wrong_token = post(
            server.url(),
            &[("Authorization", "Bearer wrong".to_string())],
            list.clone(),
        )
        .await;
        assert_eq!(wrong_token.status, 401);

        let mut from_page = authorized(&server);
        from_page.push(("Origin", "https://example.com".to_string()));
        let from_page = post(server.url(), &from_page, list).await;
        assert_eq!(from_page.status, 403);
    }
}
