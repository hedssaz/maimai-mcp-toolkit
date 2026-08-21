use std::{
    error::Error,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const FRAGMENTS: [&str; 7] = [
    include_str!("../../../contracts/public/catalog.json"),
    include_str!("../../../contracts/public/identity.json"),
    include_str!("../../../contracts/public/oauth.json"),
    include_str!("../../../contracts/public/rankings.json"),
    include_str!("../../../contracts/public/render.json"),
    include_str!("../../../contracts/public/score_query.json"),
    include_str!("../../../contracts/public/scores.json"),
];

#[tokio::test(flavor = "current_thread")]
async fn unified_public_process_routes_exact_surface_and_reopens_state() -> TestResult {
    let temp = tempfile::tempdir()?;
    let database = temp.path().join("state.db");
    let (mut child, mut stdin, mut stdout) = spawn(temp.path(), &database)?;
    initialize(&mut stdin, &mut stdout)?;

    send(
        &mut stdin,
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    )?;
    let listed = receive(&mut stdout)?;
    let tools = listed["result"]["tools"]
        .as_array()
        .ok_or("public tools missing")?;
    let expected = expected_tools()?;
    assert_eq!(tools.len(), 60);
    assert_eq!(
        tools
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>(),
        expected
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>()
    );
    for (actual, frozen) in tools.iter().zip(&expected) {
        assert_eq!(actual["description"], frozen["description"]);
        assert_eq!(actual["inputSchema"], frozen["inputSchema"]);
    }
    assert_public_schema_boundaries(tools)?;

    let mut id = 10_u64;
    for tool in tools {
        let name = tool["name"].as_str().ok_or("tool name missing")?;
        let arguments = route_probe_arguments(name);
        send(
            &mut stdin,
            &json!({
                "jsonrpc":"2.0","id":id,"method":"tools/call",
                "params":{"name":name,"arguments":arguments}
            }),
        )?;
        let response = receive(&mut stdout)?;
        let rendered = response.to_string();
        assert!(!rendered.contains("Unknown tool"), "{name}: {rendered}");
        assert!(!rendered.contains("unreachable"), "{name}: {rendered}");
        id += 1;
    }

    assert_success(
        &mut stdin,
        &mut stdout,
        id,
        "search_maimai_songs",
        json!({"id":383,"limit":1,"format":"json"}),
    )?;
    id += 1;
    assert_success(
        &mut stdin,
        &mut stdout,
        id,
        "score_counts",
        json!({"counts":{"tap":{"critical":1}}}),
    )?;
    id += 1;
    let oauth = assert_success(
        &mut stdin,
        &mut stdout,
        id,
        "maimai_lxns_status",
        json!({"qq":"10001"}),
    )?;
    assert_eq!(oauth["result"]["structuredContent"]["bound"], false);
    id += 1;
    assert_success(
        &mut stdin,
        &mut stdout,
        id,
        "qq_identity_cache_status",
        json!({}),
    )?;
    id += 1;
    let rendered = assert_success(
        &mut stdin,
        &mut stdout,
        id,
        "render_maimai_music_info",
        json!({"music_id":"383","songType":"standard"}),
    )?;
    let render_text = rendered["result"]["content"][0]["text"]
        .as_str()
        .ok_or("render result text missing")?;
    assert!(render_text.contains(".png"));
    id += 1;
    assert_success(
        &mut stdin,
        &mut stdout,
        id,
        "developer_token_status",
        json!({}),
    )?;
    id += 1;
    let score_error = call(&mut stdin, &mut stdout, id, "query_b50", json!({}))?;
    assert_ne!(score_error["result"]["isError"], false);

    finish(&mut child, stdin, stdout)?;
    assert!(database.is_file());
    assert_public_database_schema(&database).await?;

    let (mut reopened, reopened_stdin, reopened_stdout) = spawn(temp.path(), &database)?;
    let mut reopened_stdin = reopened_stdin;
    let mut reopened_stdout = reopened_stdout;
    initialize(&mut reopened_stdin, &mut reopened_stdout)?;
    finish(&mut reopened, reopened_stdin, reopened_stdout)
}

async fn assert_public_database_schema(database: &Path) -> TestResult {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(database)
        .read_only(true);
    let pool = sqlx::SqlitePool::connect_with(options).await?;
    let tables = sqlx::query_scalar::<_, String>(
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )
    .fetch_all(&pool)
    .await?;
    pool.close().await;
    assert!(!tables.is_empty());
    let private_migrations = ["maimai", "extended", "migrations"].join("_");
    for table in tables {
        assert!(!table.starts_with("official_cn_"), "private table: {table}");
        assert_ne!(table, "user_bindings");
        assert_ne!(table, private_migrations);
    }
    Ok(())
}

fn spawn(temp: &Path, database: &Path) -> TestResult<(Child, ChildStdin, BufReader<ChildStdout>)> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..");
    let mut child = Command::new(env!("CARGO_BIN_EXE_maimai-public"))
        .env_clear()
        .env("MAIMAI_DATA_DIR", root.join("data"))
        .env("MAIMAI_STATE_DB", database)
        .env(
            "MAIMAIDX_STATIC_DIR",
            root.join("maimaidx_render_mcp/static"),
        )
        .env("MAIMAIDX_COVER_CACHE_DIR", temp.join("cover-cache"))
        .env("MAIMAIDX_RENDER_OUTPUT_DIR", temp.join("images"))
        .env("B50_IMAGE_STYLE_CONFIG", temp.join("style.json"))
        .env("NAPCAT_BASE_URL", "http://127.0.0.1:9/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdin = child.stdin.take().ok_or("public stdin missing")?;
    let stdout = child.stdout.take().ok_or("public stdout missing")?;
    Ok((child, stdin, BufReader::new(stdout)))
}

fn initialize(stdin: &mut impl Write, stdout: &mut impl BufRead) -> TestResult {
    send(
        stdin,
        &json!({
            "jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{
                "protocolVersion":"2025-06-18","capabilities":{},
                "clientInfo":{"name":"public-unified-test","version":"1.0.0"}
            }
        }),
    )?;
    let initialized = receive(stdout)?;
    assert_eq!(initialized["result"]["serverInfo"]["name"], "maimai-public");
    assert_eq!(
        initialized["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    send(
        stdin,
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
}

fn route_probe_arguments(name: &str) -> Value {
    match name {
        "refresh_maimai_sources" => json!({"sources":["invalid-source"]}),
        "refresh_qq_identity_cache" => json!({"timeoutMs":0}),
        _ => json!({}),
    }
}

fn expected_tools() -> TestResult<Vec<Value>> {
    let mut tools = Vec::new();
    for fragment in FRAGMENTS {
        let parsed: Value = serde_json::from_str(fragment)?;
        tools.extend(
            parsed["tools"]
                .as_array()
                .ok_or("fragment tools missing")?
                .iter()
                .cloned(),
        );
    }
    Ok(tools)
}

fn assert_public_schema_boundaries(tools: &[Value]) -> TestResult {
    for forbidden in ["switch_score_source", "switch_b50_source"] {
        assert!(tools.iter().all(|tool| tool["name"] != forbidden));
    }
    for name in ["render_maimai_score_list", "render_maimai_rise_score"] {
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == name)
            .ok_or("public render tool missing")?;
        let properties = tool["inputSchema"]["properties"]
            .as_object()
            .ok_or("public render properties missing")?;
        for forbidden in [
            "source",
            "scoreSource",
            "score_source",
            "dataSource",
            "data_source",
        ] {
            assert!(
                !properties.contains_key(forbidden),
                "{name} exposed {forbidden}"
            );
        }
    }
    Ok(())
}

fn assert_success(
    stdin: &mut impl Write,
    stdout: &mut impl BufRead,
    id: u64,
    name: &str,
    arguments: Value,
) -> TestResult<Value> {
    let response = call(stdin, stdout, id, name, arguments)?;
    assert_eq!(response["result"]["isError"], false, "{name}: {response}");
    Ok(response)
}

fn call(
    stdin: &mut impl Write,
    stdout: &mut impl BufRead,
    id: u64,
    name: &str,
    arguments: Value,
) -> TestResult<Value> {
    send(
        stdin,
        &json!({
            "jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":name,"arguments":arguments}
        }),
    )?;
    receive(stdout)
}

fn finish(child: &mut Child, stdin: ChildStdin, mut stdout: BufReader<ChildStdout>) -> TestResult {
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let killed = child.wait()?;
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                pipe.read_to_string(&mut stderr)?;
            }
            return Err(format!(
                "public stdio did not exit within 10s; killed as {killed}: {stderr}"
            )
            .into());
        }
        thread::sleep(Duration::from_millis(25));
    };
    let mut trailing = String::new();
    stdout.read_to_string(&mut trailing)?;
    assert!(trailing.trim().is_empty(), "unexpected stdout: {trailing}");
    if !status.success() {
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            pipe.read_to_string(&mut stderr)?;
        }
        return Err(format!("public stdio exited with {status}: {stderr}").into());
    }
    Ok(())
}

fn send(writer: &mut impl Write, value: &Value) -> TestResult {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn receive(reader: &mut impl BufRead) -> TestResult<Value> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err("public child response missing".into());
    }
    Ok(serde_json::from_str(&line)?)
}
