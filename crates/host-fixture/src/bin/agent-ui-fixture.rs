use agent_protocol::{
    browser::{BrowserAction, BrowserRequest},
    session::{ProviderKind, SessionRef},
};
use host_fixture::{
    pairing::PairingServer,
    test_support::{HostFixture, Memory},
};
use std::{path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() {
    let directory = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("isolated directory required"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let directory = dunce::canonicalize(&directory).unwrap();
    agent_transport::diagnostics::initialize(
        &directory,
        agent_transport::diagnostics::Component::Host,
        env!("CARGO_PKG_VERSION"),
    )
    .unwrap();
    let workspace = directory.join("project");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(
        workspace.join("hello.txt"),
        "日本語のファイル\n編集を確認します。\n",
    )
    .unwrap();
    let nested = workspace.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        nested.join("child.txt"),
        "Nested directory navigation fixture.\n",
    )
    .unwrap();
    std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&workspace)
        .status()
        .unwrap();
    let fixture = PathBuf::from(
        std::env::args_os()
            .nth(2)
            .expect("Codex fixture executable required"),
    );
    let port_file = std::env::args_os()
        .nth(3)
        .expect("port output file required");
    let program = if fixture == std::path::Path::new("--without-codex") {
        directory.join("missing-codex")
    } else {
        host_fixture::fixture::Config {
            trace: true,
            stream_delay_ms: std::env::args()
                .nth(4)
                .map(|value| value.parse().expect("stream delay must be milliseconds"))
                .unwrap_or(8000),
            ..Default::default()
        }
        .install(&fixture, &directory)
        .unwrap()
    };
    let projects = directory.join("bex-projects.json");
    std::fs::write(&projects,serde_json::to_vec(&serde_json::json!([{"id":"simulator-project","name":"検証プロジェクト","roots":[{"path":workspace}],"createdAt":1,"updatedAt":1}])).unwrap()).unwrap();
    std::fs::write(directory.join("account-fixture.json"), r#"{"type":"chatgpt","email":"desktop@example.invalid","planType":"plus","accountId":"desktop"}"#).unwrap();
    let config = codex_app_server::AppServerConfig {
        program,
        ..Default::default()
    };
    let host = HostFixture::start(
        &directory,
        config,
        Arc::new(Memory::default()),
        "検証 Host",
        true,
        std::env::args_os().nth(5).map(PathBuf::from).as_deref(),
    )
    .await
    .unwrap();
    #[cfg(target_os = "macos")]
    let mut browser_desktop = None;
    if let Some(argument) = std::env::args_os().nth(6) {
        assert_eq!(argument, "--prepare-browser");
        // Use the production native owner in an isolated desktop window. The
        // fixture's authenticated Host RPC reaches exactly that renderer.
        #[cfg(target_os = "macos")]
        {
            let executable = std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .join("agent-desktop");
            let mut child = tokio::process::Command::new(executable)
                .env("AGENT_STATE_DIR", &directory)
                .env("AGENT_ISOLATED_HOST", "1")
                .env("AGENT_HOST_DAEMON", "/usr/bin/false")
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
            while agent_local_host::browser::read_renderer(&directory).is_err() {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "native browser desktop exited during startup"
                );
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "native browser renderer did not become ready"
                );
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            browser_desktop = Some(child);
        }
        println!("Preparing browser dependency before Simulator startup");
        let connection = host.local().await.unwrap();
        let ready = connection
            .peer
            .call(&BrowserRequest {
                thread_id: SessionRef::new(ProviderKind::Codex, "browser-preflight".into())
                    .unwrap(),
                tab_id: String::new(),
                image_id: String::new(),
                renderer_id: String::new(),
                generation: 0,
                action: BrowserAction::Read,
            })
            .await;
        connection.close().await;
        ready.expect("isolated browser dependency must be ready before Simulator startup");
        println!("Browser dependency ready");
    }
    let pairing = PairingServer::start(
        &directory,
        host.ticket.clone(),
        host.credentials.local_identity().await,
    )
    .unwrap();
    std::fs::write(port_file, pairing.port.to_string()).unwrap();
    println!("UI fixture ready: {}", directory.display());
    tokio::signal::ctrl_c().await.unwrap();
    pairing.shutdown().unwrap();
    host.close().await.unwrap();
    #[cfg(target_os = "macos")]
    if let Some(mut child) = browser_desktop {
        let _ = child.kill().await;
    }
}
