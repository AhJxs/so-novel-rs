//! 流式下载发布包到暂存文件，边下边回报字节进度。
//!
//! 请求用调用方传入的 client（走 `gh_proxy` / 系统代理与 TLS 配置），但用单请求 timeout
//! 覆盖 client 默认的 10s —— 发布包几十 MB，默认超时必然中断。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt as _;

use crate::core::update::UpdateError;
use crate::crawler::download_options::CancelToken;

/// 单次下载允许的最长时间。
const DOWNLOAD_TIMEOUT: Duration = Duration::from_mins(30);
/// 进度回报的最小间隔，避免每 8 KB 就唤醒一次 UI。
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// 后台下载任务回报给 UI 的事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadEvent {
    /// 已接收 / 总字节数（总数为 0 表示服务器没给长度）。
    Progress { received: u64, total: u64 },
    /// 下载完成，`path` 为落盘位置。
    Finished { path: PathBuf },
    /// 用户取消。
    Cancelled,
    /// 下载失败（含大小不符）。
    Failed(String),
}

/// 一次下载请求。
pub struct DownloadRequest<'a> {
    /// 产物直链。
    pub url: &'a str,
    /// release API 给出的字节数；0 表示未知，退化为响应头 `Content-Length`。
    pub expected_size: u64,
    /// 落盘路径（父目录需已存在）。
    pub dest: PathBuf,
}

/// 下载到 `request.dest`，成功返回该路径。
///
/// `on_progress` 在下载线程上同步调用，频率不超过 [`PROGRESS_INTERVAL`]。
/// 失败或取消时删除半成品文件。
///
/// # Errors
///
/// HTTP 状态非 2xx、网络中断、用户取消、或实际字节数与 `expected_size` 不符时返回
/// [`UpdateError`]。
pub async fn download_to_file(
    client: &reqwest::Client,
    request: DownloadRequest<'_>,
    cancel: &CancelToken,
    on_progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<PathBuf, UpdateError> {
    match download_inner(client, &request, cancel, on_progress).await {
        Ok(()) => Ok(request.dest),
        Err(e) => {
            let _ = tokio::fs::remove_file(&request.dest).await;
            Err(e)
        }
    }
}

async fn download_inner(
    client: &reqwest::Client,
    request: &DownloadRequest<'_>,
    cancel: &CancelToken,
    on_progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<(), UpdateError> {
    if cancel.is_cancelled() {
        return Err(UpdateError("已取消下载".into()));
    }
    let mut resp = client
        .get(request.url)
        .header(reqwest::header::USER_AGENT, "so-novel-rs")
        // 不让中间层压缩，否则收到的字节数与 asset.size 对不上。
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .timeout(DOWNLOAD_TIMEOUT)
        .send()
        .await
        .map_err(|e| UpdateError(format!("下载请求失败: {e}")))?;
    if !resp.status().is_success() {
        return Err(UpdateError(format!("下载返回 HTTP {}", resp.status())));
    }

    let total = if request.expected_size > 0 {
        request.expected_size
    } else {
        resp.content_length().unwrap_or(0)
    };
    on_progress(0, total);

    let mut file = tokio::fs::File::create(&request.dest)
        .await
        .map_err(|e| UpdateError(format!("创建临时文件失败: {e}")))?;
    let mut received = 0u64;
    let mut last_report = Instant::now();

    loop {
        if cancel.is_cancelled() {
            return Err(UpdateError("已取消下载".into()));
        }
        let chunk = resp
            .chunk()
            .await
            .map_err(|e| UpdateError(format!("下载中断: {e}")))?;
        let Some(chunk) = chunk else { break };
        file.write_all(&chunk)
            .await
            .map_err(|e| UpdateError(format!("写入临时文件失败: {e}")))?;
        received += chunk.len() as u64;
        if last_report.elapsed() >= PROGRESS_INTERVAL {
            last_report = Instant::now();
            on_progress(received, total);
        }
    }
    file.flush()
        .await
        .map_err(|e| UpdateError(format!("写入临时文件失败: {e}")))?;
    file.sync_all()
        .await
        .map_err(|e| UpdateError(format!("落盘失败: {e}")))?;
    on_progress(received, total);

    if request.expected_size > 0 && received != request.expected_size {
        return Err(UpdateError(format!(
            "下载大小不符: 收到 {received} 字节, 预期 {} 字节",
            request.expected_size
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use std::sync::Mutex;

    use tokio::io::AsyncReadExt as _;
    use tokio::net::TcpListener;

    use super::*;

    /// 起一个只响应一次的最小 HTTP 服务，返回 (url, `JoinHandle`)。
    async fn serve_once(
        status: &'static str,
        body: &'static [u8],
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await.unwrap();
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(body).await.unwrap();
        });
        (format!("http://{addr}/pkg.zip"), task)
    }

    #[tokio::test]
    async fn downloads_body_and_reports_progress() {
        let (url, server) = serve_once("200 OK", b"hello-world").await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("pkg.zip");
        let seen = Mutex::new(Vec::new());

        let path = download_to_file(
            &reqwest::Client::new(),
            DownloadRequest {
                url: &url,
                expected_size: 11,
                dest: dest.clone(),
            },
            &CancelToken::new(),
            &|received, total| seen.lock().unwrap().push((received, total)),
        )
        .await
        .expect("下载应成功");

        server.await.unwrap();
        assert_eq!(path, dest);
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello-world");
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.first(), Some(&(0, 11)));
        assert_eq!(seen.last(), Some(&(11, 11)));
    }

    #[tokio::test]
    async fn size_mismatch_removes_file() {
        let (url, server) = serve_once("200 OK", b"short").await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("pkg.zip");

        let err = download_to_file(
            &reqwest::Client::new(),
            DownloadRequest {
                url: &url,
                expected_size: 1024,
                dest: dest.clone(),
            },
            &CancelToken::new(),
            &|_, _| {},
        )
        .await
        .expect_err("大小不符应失败");

        server.await.unwrap();
        assert!(err.to_string().contains("大小不符"));
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn http_error_is_reported() {
        let (url, server) = serve_once("404 Not Found", b"nope").await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("pkg.zip");

        let err = download_to_file(
            &reqwest::Client::new(),
            DownloadRequest {
                url: &url,
                expected_size: 0,
                dest: dest.clone(),
            },
            &CancelToken::new(),
            &|_, _| {},
        )
        .await
        .expect_err("404 应失败");

        server.await.unwrap();
        assert!(err.to_string().contains("404"));
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn cancelled_token_aborts_before_network() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("pkg.zip");
        let cancel = CancelToken::new();
        cancel.cancel();

        let err = download_to_file(
            &reqwest::Client::new(),
            DownloadRequest {
                url: "http://127.0.0.1:1/never",
                expected_size: 0,
                dest: dest.clone(),
            },
            &cancel,
            &|_, _| {},
        )
        .await
        .expect_err("已取消应立即失败");

        assert!(err.to_string().contains("取消"));
        assert!(!dest.exists());
    }
}
