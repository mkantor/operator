//! This module contains various types of steaming HTTP response bodies
//! (reading from files, capturing stdout of a process, etc). All of these
//! types have an impl for Stream<Item=Result<Bytes, StreamError>>.

use super::StreamError;
use bytes::Bytes;
use futures::future::{Future, FutureExt, LocalBoxFuture};
use futures::Stream;
use std::cmp;
use std::fs::File;
use std::io::ErrorKind::Interrupted;
use std::io::{self, Read, Seek};
use std::mem;
use std::pin::Pin;
use std::process::Child;
use std::task::{Context, Poll};

// FIXME: Should not depend on actix from inside the content module.
use actix_web::error::BlockingError;
use actix_web::web;

type ChunkOperation<'a, T> = LocalBoxFuture<'a, Result<T, BlockingError<StreamError>>>;

fn handle_error(error: BlockingError<StreamError>) -> StreamError {
    match error {
        BlockingError::Error(error) => error,
        BlockingError::Canceled => StreamError::Canceled,
    }
}

/// The simplest body: just some bytes that are already in memory. This is a
/// poor excuse for a "stream" (it just dumps everything on the first poll and
/// then ends) but it's handy so we can always work with streams, even at the
/// cost of some efficiency.
pub struct InMemoryBody(pub Bytes);
impl Stream for InMemoryBody {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context) -> Poll<Option<Self::Item>> {
        let bytes = mem::take(&mut self.0);
        Poll::Ready(if bytes.is_empty() {
            None
        } else {
            Some(Ok(bytes))
        })
    }
}

/// HTTP response body populated by a local file. This was yoinked [from
/// actix-files's `ChunkedReadFile`](https://github.com/actix/actix-web/blob/web-v3.0.0-beta.3/actix-files/src/lib.rs#L58-L117)
/// and only lightly modified.
pub struct FileBody {
    size: u64,
    offset: u64,
    file: Option<File>,
    next: Option<ChunkOperation<'static, (File, Bytes)>>,
    counter: u64,
}
impl FileBody {
    pub fn try_from_file(file: File) -> Result<Self, io::Error> {
        Ok(Self {
            size: file.metadata()?.len(),
            offset: 0,
            file: Some(file),
            next: None,
            counter: 0,
        })
    }
}
impl Stream for FileBody {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context) -> Poll<Option<Self::Item>> {
        if let Some(ref mut future) = self.next {
            return match Pin::new(future).poll(context) {
                Poll::Ready(Ok((file, bytes))) => {
                    self.next.take();
                    self.file = Some(file);
                    self.offset += bytes.len() as u64;
                    self.counter += bytes.len() as u64;
                    Poll::Ready(Some(Ok(bytes)))
                }
                Poll::Ready(Err(error)) => Poll::Ready(Some(Err(handle_error(error)))),
                Poll::Pending => Poll::Pending,
            };
        }

        let size = self.size;
        let offset = self.offset;
        let counter = self.counter;

        if size == counter {
            Poll::Ready(None)
        } else {
            let mut file = self.file.take().expect("Use after completion");
            self.next = Some(
                web::block(move || {
                    let max_bytes = cmp::min(size.saturating_sub(counter), 65_536);
                    let mut buffer = Vec::with_capacity(max_bytes as usize);
                    file.seek(io::SeekFrom::Start(offset))?;
                    file.by_ref().take(max_bytes).read_to_end(&mut buffer)?;
                    Ok((file, Bytes::from(buffer)))
                })
                .boxed_local(),
            );
            self.poll_next(context)
        }
    }
}

/// HTTP response body populated from the stdout of a running process.
pub struct ProcessBody {
    process: Option<Child>,
    next: Option<ChunkOperation<'static, (Option<Child>, Bytes)>>,
}
impl ProcessBody {
    pub fn new(process: Child) -> Self {
        ProcessBody {
            process: Some(process),
            next: None,
        }
    }
}
impl Stream for ProcessBody {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context) -> Poll<Option<Self::Item>> {
        if let Some(ref mut future) = self.next {
            return match Pin::new(future).poll(context) {
                Poll::Ready(Ok((process, bytes))) => {
                    self.next.take();
                    self.process = process;
                    Poll::Ready(Some(Ok(bytes)))
                }
                Poll::Ready(Err(e)) => {
                    self.process = None; // Give up on the process after hitting an error.
                    Poll::Ready(Some(Err(handle_error(e))))
                }
                Poll::Pending => Poll::Pending,
            };
        }

        let mut process = match self.process.take() {
            // None means the process has terminated; we're all done!
            None => return Poll::Ready(None),
            Some(process) => process,
        };

        let pid = process.id();
        let next = web::block(move || {
            let mut buffer = [0; 32]; // FIXME: 32 bytes is totally arbitrary.
            match process.stdout {
                None => Err(StreamError::ExecutableOutputCouldNotBeCaptured { pid }),
                Some(ref mut stdout) => {
                    match stdout.read(&mut buffer) {
                        Err(error) if error.kind() == Interrupted => {
                            // If the read was interrupted then it can be tried
                            // again on the next poll. Just emit an empty chunk.
                            Ok((Some(process), Bytes::new()))
                        }
                        Err(fatal_error) => Err(StreamError::from(fatal_error)),
                        Ok(0) => {
                            match process.try_wait()? {
                                None => {
                                    // The process is still running, there was just
                                    // no new output.
                                    Ok((Some(process), Bytes::new()))
                                }
                                Some(exit_status) => {
                                    if !exit_status.success() {
                                        let stderr_contents = {
                                            process.stderr.and_then(|mut stderr| {
                                                let mut error_message = String::new();
                                                match stderr.read_to_string(&mut error_message) {
                                                    Err(_) | Ok(0) => None,
                                                    Ok(_) => Some(error_message),
                                                }
                                            })
                                        };

                                        Err(StreamError::ExecutableExitedWithNonzero {
                                            pid,
                                            stderr_contents,
                                            exit_code: exit_status.code(),
                                        })
                                    } else {
                                        // Successful completion.
                                        Ok((None, Bytes::new()))
                                    }
                                }
                            }
                        }
                        Ok(size) => Ok((Some(process), Bytes::copy_from_slice(&buffer[..size]))),
                    }
                }
            }
        })
        .boxed_local();

        self.next = Some(next);
        self.poll_next(context)
    }
}
