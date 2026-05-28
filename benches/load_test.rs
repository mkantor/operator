use actix_rt::SystemRunner;
use awc::error::PayloadError;
use awc::http::StatusCode;
use awc::{Client as HttpClient, ClientResponse};
use bytes::{Bytes, BytesMut};
use criterion::{BenchmarkId, Criterion, criterion_main};
use futures::FutureExt;
use futures::{Stream, TryStreamExt, future};
use lazy_static::lazy_static;
use mime_guess::MimeGuess;
use operator::content::ContentDirectory;
use operator::content::Route;
use operator::test_lib::*;
use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::net::{SocketAddr, TcpListener};
use std::process::{Child, Command, Stdio};
use std::str;
use std::thread;
use std::time;

fn unused_addr() -> SocketAddr {
    TcpListener::bind("127.0.0.1:0")
        .expect("Could not bind to find an unused port")
        .local_addr()
        .expect("Could not get local_addr from TcpListener")
}

const CONCURRENT_REQUESTS_PER_ROUTE: u8 = 10;

lazy_static! {
    static ref BENCHMARKED_SAMPLES: HashMap<&'static str, ContentDirectory> = [
        "empty",
        "hello-world",
        "realistic-basic",
        "realistic-advanced",
    ]
    .iter()
    .map(|name| (*name, sample_content_directory(name)))
    .collect();
}

criterion_main!(benchmark_all_samples);

fn benchmark_all_samples() {
    let mut criterion = Criterion::default()
        .noise_threshold(0.1)
        .sample_size(10)
        .configure_from_args();
    let runtime = actix_rt::System::new();

    for (sample_name, content_directory) in BENCHMARKED_SAMPLES.iter() {
        let server = RunningServer::start(&content_directory).expect("Server failed to start");
        criterion.bench_with_input(
            BenchmarkId::new("load-test", sample_name),
            &content_directory,
            |bencher, content_directory| {
                benchmark_load_test(bencher, &runtime, content_directory, &server)
            },
        );
    }
}

fn benchmark_load_test(
    bencher: &mut criterion::Bencher,
    runtime: &SystemRunner,
    content_directory: &'static ContentDirectory,
    server: &RunningServer,
) {
    bencher.iter(|| runtime.block_on(load_test(content_directory, *server.address())))
}

async fn load_test(content_directory: &'static ContentDirectory, server_address: SocketAddr) {
    // This contains some Rc'd constituents, so cloning it is cheaper than
    // allocating a new client for every request.
    let http_client = HttpClient::new();

    let requests = content_directory.into_iter().flat_map(|content_file| {
        let empty_string = String::from("");
        let first_filename_extension = content_file.extensions.first().unwrap_or(&empty_string);

        // Target media type is just the source media type.
        let target_media_type = MimeGuess::from_ext(first_filename_extension)
            .first()
            .unwrap_or(mime::STAR_STAR);

        let mut requests_for_this_route =
            Vec::with_capacity(CONCURRENT_REQUESTS_PER_ROUTE as usize);
        for _ in 0..CONCURRENT_REQUESTS_PER_ROUTE {
            let target_media_type = target_media_type.clone();
            let http_client = http_client.clone();
            requests_for_this_route.push(async move {
                render_via_http_request(
                    &http_client,
                    &server_address,
                    &content_file.route,
                    &target_media_type.to_string(),
                )
                .map(|result| result.1.expect("Payload error"))
                .await
            });
        }
        requests_for_this_route
    });

    future::join_all(requests).await;
}

// TODO: Would be nice to share the below utils with the integration tests.

fn operator_command<I, S>(args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let target_dir = env::current_exe()
        .ok()
        .map(|mut path| {
            path.pop();
            if path.ends_with("deps") {
                path.pop();
            }
            path
        })
        .unwrap();

    let bin_path = target_dir.join(format!("operator{}", env::consts::EXE_SUFFIX));

    let mut operator = Command::new(bin_path);
    operator.args(args);
    operator
}

struct RunningServer {
    address: SocketAddr,
    process: Child,
}

impl RunningServer {
    fn start(content_directory: &ContentDirectory) -> Result<Self, String> {
        let address = unused_addr();

        let mut command = operator_command(&[
            "serve",
            "--quiet",
            &format!(
                "--content-directory={}",
                content_directory
                    .root()
                    .to_str()
                    .expect("Content directory root path was not UTF-8")
            ),
            &format!("--bind-to={}", address),
        ]);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        let mut process = command.spawn().expect("Failed to spawn process");

        // Give the server a chance to start up.
        // TODO: It would be better to poll by retrying a few times if the
        // connection is refused.
        thread::sleep(time::Duration::from_secs(1));

        // The server may have failed to start if the content directory was invalid.
        if let Ok(Some(_)) = process.try_wait() {
            Err(match process.wait_with_output() {
                Err(error) => format!(
                    "Server for {} failed to start and output is unavailable: {}",
                    content_directory.root().to_string_lossy(),
                    error,
                ),
                Ok(output) => format!(
                    "Server for {} failed to start: {}",
                    content_directory.root().to_string_lossy(),
                    String::from_utf8_lossy(&output.stderr),
                ),
            })
        } else {
            Ok(RunningServer { address, process })
        }
    }

    fn address(&self) -> &SocketAddr {
        &self.address
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.process.kill().expect("Failed to kill server")
    }
}

async fn render_via_http_request(
    http_client: &HttpClient,
    server_address: &SocketAddr,
    route: &Route,
    accept: &str,
) -> (StatusCode, Result<Bytes, PayloadError>) {
    let request = http_client
        .get(format!("http://{}{}", server_address, route))
        .insert_header(("Accept", accept))
        .timeout(time::Duration::from_secs(30));

    match request.send().await {
        Err(send_request_error) => panic!(
            "Failed while sending request for http://{}{}: {}",
            server_address, route, send_request_error,
        ),
        Ok(response) => {
            let response_status = response.status();
            let response_body = collect_response_body(response).await;
            (response_status, response_body)
        }
    }
}

async fn collect_response_body<S>(response: ClientResponse<S>) -> Result<Bytes, PayloadError>
where
    S: Stream<Item = Result<Bytes, PayloadError>> + Unpin,
{
    response
        .try_fold(BytesMut::new(), |mut accumulator, bytes| {
            accumulator.extend_from_slice(&bytes);
            let max_length = 64;
            if bytes.len() > max_length {
                log::trace!("HTTP client accumulated {:?}... and {} more bytes for response body ({} bytes collected so far)",
                bytes.slice(0..max_length),
                bytes.len() - max_length, accumulator.len());
            } else {
                log::trace!("HTTP client accumulated {:?} for response body ({} bytes collected so far)", bytes, accumulator.len());
            }
            async { Ok(accumulator) }
        })
        .await
        .map(|bytes| {
            log::trace!("HTTP client finished accumulating response body ({} bytes total)", bytes.len());
            bytes.freeze()
        })
        .map_err(|error| {
            log::error!("HTTP client encountered an error error while accumulating response body: {}", error);
            error
        })
}
