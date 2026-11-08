//! A server's connection information over the user's own SSH: its Bex Host
//! issues an invitation, which this device then confirms and pairs with as a
//! pasted one.
use crate::l10n::tr;
use agent_protocol::models::Invitation;
use std::{process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;

/// The most an invitation's JSON can take; more is not Bex's answer.
const LIMIT: u64 = 64 * 1024;

pub(super) async fn invitation(destination: &str) -> Result<Invitation, String> {
    let destination = agent_core::presentation::connections::validate_ssh_destination(destination)
        .map_err(|reason| tr(reason).to_owned())?;
    // Keys only, known servers only: SSH never asks for a password or trusts a
    // new key here. `--` keeps the destination from being read as an option.
    let mut command = tokio::process::Command::new("ssh");
    command
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ConnectTimeout=10",
            "--",
            destination,
            "sh -lc 'exec host-daemon invite'",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashes.
    let mut child = command.spawn().map_err(|_| {
        tr("SSHを起動できません。SSHがインストールされているか確認してください。").to_owned()
    })?;
    let mut stdout = child.stdout.take().expect("SSH stdout is piped");
    let answer = tokio::time::timeout(Duration::from_secs(20), async {
        let mut contents = Vec::new();
        (&mut stdout)
            .take(LIMIT + 1)
            .read_to_end(&mut contents)
            .await
            .map_err(|_| tr("SSHの応答を読み取れません。"))?;
        if contents.len() as u64 > LIMIT {
            return Err(tr("SSHの応答が長すぎます。"));
        }
        let status = child
            .wait()
            .await
            .map_err(|_| tr("SSHの終了を確認できません。"))?;
        if !status.success() {
            return Err(tr(
                "接続できません。SSHの鍵と接続先を確認し、サーバーでBexのホストを起動してください。",
            ));
        }
        serde_json::from_slice(&contents).map_err(|_| {
            tr("Bexの接続情報を取得できません。接続先で host-daemon invite を実行できるか確認してください。")
        })
    })
    .await;
    match answer {
        Ok(answer) => answer.map_err(str::to_owned),
        Err(_) => Err(tr("SSH接続がタイムアウトしました。").into()),
    }
}
