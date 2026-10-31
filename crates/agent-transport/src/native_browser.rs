//! One operation per private local connection. A deadline drops the requesting
//! connection; it neither destroys native pages nor replays uncertain input.
use agent_protocol::{
    browser_native::{NativeReply, NativeRequest},
    protocol,
};
use serde::{Serialize, de::DeserializeOwned};
use std::{io, path::Path, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::UnixStream,
};

pub async fn write<W: AsyncWrite + Unpin, T: Serialize>(
    stream: &mut W,
    value: &T,
) -> io::Result<()> {
    let bytes = protocol::encode(value).map_err(io::Error::other)?;
    if bytes.len() > protocol::MAX_FRAME_BYTES {
        return Err(io::Error::other("browser message is too large"));
    }
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await
}

pub async fn read<R: AsyncRead + Unpin, T: DeserializeOwned>(stream: &mut R) -> io::Result<T> {
    let length = stream.read_u32().await? as usize;
    if length > protocol::MAX_FRAME_BYTES {
        return Err(io::Error::other("browser message is too large"));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await?;
    protocol::decode(&bytes).map_err(io::Error::other)
}

pub async fn request(socket: &Path, command: &NativeRequest) -> NativeReply {
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        let mut stream = UnixStream::connect(socket).await.map_err(|_| "内蔵ブラウザに接続できません。PC側のBexでウィンドウを開いてください。".to_string())?;
        let interrupted="ブラウザ操作の応答が途切れました。操作が完了している可能性があるため、画面を確認してください。";
        write(&mut stream, command).await.map_err(|_|interrupted.to_string())?;
        read::<_, NativeReply>(&mut stream).await.map_err(|_|interrupted.to_string())
    })
    .await;
    match result {
        Ok(Ok(reply)) => reply,
        Ok(Err(error)) => Err(error),
        Err(_) => Err("ブラウザ操作の応答がタイムアウトしました。操作が完了している可能性があるため、画面を確認してください。".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::browser::BrowserAction;
    use agent_protocol::browser_native::{Capture, NativeAction};

    #[tokio::test]
    async fn losing_a_reply_does_not_replay_input_or_stop_later_reads() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("browser.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let input: NativeRequest = read(&mut stream).await.unwrap();
            assert!(matches!(
                input.action,
                NativeAction::Browser(BrowserAction::Type { .. })
            ));
            drop(stream);
            let (mut stream, _) = listener.accept().await.unwrap();
            let input: NativeRequest = read(&mut stream).await.unwrap();
            assert!(matches!(
                input.action,
                NativeAction::Browser(BrowserAction::Read)
            ));
            write(
                &mut stream,
                &Ok::<_, String>(agent_protocol::browser::BrowserFrame {
                    tab_id: "retained".into(),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        });
        let mut command = NativeRequest {
            scope: "thread".into(),
            displayed: None,
            image_id: String::new(),
            renderer_id: String::new(),
            capture: Capture::None,
            action: NativeAction::Browser(BrowserAction::Type {
                text: "once".into(),
            }),
        };
        assert!(
            request(&path, &command)
                .await
                .unwrap_err()
                .contains("完了している可能性")
        );
        command.action = NativeAction::Browser(BrowserAction::Read);
        assert_eq!(request(&path, &command).await.unwrap().tab_id, "retained");
        server.await.unwrap();
        assert!(
            request(&path, &command)
                .await
                .unwrap_err()
                .contains("PC側のBex")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_late_operation_is_sent_once_and_its_timeout_does_not_stop_the_renderer() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("browser.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let command: NativeRequest = read(&mut stream).await.unwrap();
            assert!(matches!(
                command.action,
                NativeAction::Browser(BrowserAction::Type { .. })
            ));
            tokio::time::sleep(Duration::from_secs(9)).await;
            let _ = write(
                &mut stream,
                &Ok::<_, String>(agent_protocol::browser::BrowserFrame::default()),
            )
            .await;
            let (mut stream, _) = listener.accept().await.unwrap();
            let command: NativeRequest = read(&mut stream).await.unwrap();
            assert!(matches!(
                command.action,
                NativeAction::Browser(BrowserAction::Read)
            ));
            write(
                &mut stream,
                &Ok::<_, String>(agent_protocol::browser::BrowserFrame {
                    tab_id: "kept".into(),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        });
        let mut command = NativeRequest {
            scope: "thread".into(),
            displayed: None,
            image_id: String::new(),
            renderer_id: String::new(),
            capture: Capture::None,
            action: NativeAction::Browser(BrowserAction::Type {
                text: "once".into(),
            }),
        };
        assert!(
            request(&path, &command)
                .await
                .unwrap_err()
                .contains("タイムアウト")
        );
        command.action = NativeAction::Browser(BrowserAction::Read);
        assert_eq!(request(&path, &command).await.unwrap().tab_id, "kept");
        server.await.unwrap();
    }
}
