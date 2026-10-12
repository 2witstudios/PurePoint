//! Host-side commands against a routed worker. Every request travels through
//! the local daemon's pinned link; this CLI never contacts a worker itself.
//! Mutations carry an explicit operation identity: reuse the printed window and
//! operation to retry safely, because the worker deduplicates by them.

use crate::error::CliError;
use crate::{client, output};
use clap::Subcommand;
use pu_core::mesh::{
    admin::{MeshAdminRequest, MeshAdminResponse, WorkerRoute},
    local::LocalRoute,
    response::{Body, Response as WireResponse},
    wire::{self, ControlGuard, Export, Mutation, Operation, WriterCheckpoint},
};
use pu_core::protocol::{Request, Response};
use std::path::Path;

/// Lifetime of windows the CLI issues; reuse within it to retry an operation.
const DEFAULT_WINDOW_TTL: u32 = 3600;

/// Remote run selection shared by run-scoped commands.
#[derive(Debug, clap::Args)]
pub struct RunSelector {
    #[arg(long)]
    export: String,
    #[arg(long, default_value_t = 1)]
    export_generation: u64,
    #[arg(long)]
    agent: String,
}

/// Explicit operation identity; omitted values are issued and printed.
#[derive(Debug, clap::Args)]
pub struct OperationArgs {
    #[arg(long)]
    window: Option<u64>,
    #[arg(long)]
    operation: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum RemoteAction {
    /// Exports this host may inspect on the worker
    Inventory,
    /// Durable agent/run status for one export
    Status {
        #[arg(long)]
        export: String,
        #[arg(long, default_value_t = 1)]
        export_generation: u64,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 64)]
        limit: u16,
    },
    /// Issue an operation window for retry-safe mutations
    Window {
        #[arg(long, default_value_t = DEFAULT_WINDOW_TTL)]
        ttl_seconds: u32,
    },
    /// Spawn a granted runtime against an exact, existing source commit
    Spawn {
        #[arg(long)]
        export: String,
        #[arg(long, default_value_t = 1)]
        export_generation: u64,
        #[arg(long)]
        runtime: String,
        #[arg(long)]
        commit: String,
        #[arg(long, default_value = "")]
        prompt: String,
        #[command(flatten)]
        operation: OperationArgs,
    },
    /// Acquire control of a run that has no controller
    Acquire {
        #[command(flatten)]
        run: RunSelector,
        #[command(flatten)]
        operation: OperationArgs,
    },
    /// Durably stop a run this host controls
    Stop {
        #[command(flatten)]
        run: RunSelector,
        #[command(flatten)]
        operation: OperationArgs,
    },
    /// Open the single input writer for the run's current control epoch
    Writer {
        #[command(flatten)]
        run: RunSelector,
        /// Inspected checkpoint of the writer being replaced
        #[arg(long, requires = "previous_sequence")]
        previous_writer: Option<u64>,
        #[arg(long, requires = "previous_writer")]
        previous_sequence: Option<u64>,
    },
    /// Send one input; it is never retried automatically
    Input {
        #[command(flatten)]
        run: RunSelector,
        #[arg(long)]
        writer: u64,
        #[arg(long)]
        sequence: u64,
        #[arg(long)]
        text: String,
        /// Append a carriage return (submit)
        #[arg(long)]
        submit: bool,
    },
    /// Exact acknowledgement for a previously sent input
    InputReceipt {
        #[command(flatten)]
        run: RunSelector,
        #[arg(long)]
        writer: u64,
        #[arg(long)]
        sequence: u64,
    },
    /// Resize the run's terminal
    Resize {
        #[command(flatten)]
        run: RunSelector,
        #[arg(long)]
        cols: u16,
        #[arg(long)]
        rows: u16,
        #[command(flatten)]
        operation: OperationArgs,
    },
    /// Stream the run's output (gaps are reported, never filled)
    Watch {
        #[command(flatten)]
        run: RunSelector,
        /// Resume after the last output seen, as printed when a watch stops
        /// (`<incarnation>:<offset>`); the worker reports any evicted gap.
        #[arg(long, value_parser = parse_cursor)]
        cursor: Option<wire::OutputCursor>,
    },
}

fn parse_cursor(value: &str) -> Result<wire::OutputCursor, String> {
    let (incarnation, offset) = value
        .split_once(':')
        .ok_or("expected <incarnation>:<offset>")?;
    Ok(wire::OutputCursor {
        incarnation: incarnation.to_string(),
        offset: offset.parse().map_err(|_| "offset must be a number")?,
    })
}

/// Resolve a fingerprint or unique label against this host's own routes.
pub(crate) fn select_worker(
    workers: Vec<WorkerRoute>,
    selector: &str,
) -> Result<WorkerRoute, CliError> {
    let mut matches = workers
        .into_iter()
        .filter(|worker| worker.fingerprint == selector || worker.label == selector);
    match (matches.next(), matches.next()) {
        (Some(worker), None) => Ok(worker),
        (Some(_), Some(_)) => Err(CliError::Other(
            "worker label is ambiguous; use its fingerprint".into(),
        )),
        _ => Err(CliError::Other("no routed worker matches".into())),
    }
}

struct Remote<'a> {
    socket: &'a Path,
    worker: WorkerRoute,
    json: bool,
}

/// A worker refusal, keeping its exact error code for the caller.
fn refused(what: &str, body: &Body) -> CliError {
    match body {
        Body::Error { code } => CliError::Other(format!("worker refused {what}: {code:?}")),
        _ => CliError::Other(format!("unexpected worker reply to {what}")),
    }
}

impl Remote<'_> {
    fn request(&self, operation: Operation) -> wire::Request {
        wire::Request {
            request_id: pu_core::id::session_id(),
            target_daemon: self.worker.daemon_id.clone(),
            operation,
        }
    }

    fn route(&self, operation: Operation) -> Result<LocalRoute, CliError> {
        let route = LocalRoute {
            worker: self.worker.fingerprint.clone(),
            request: self.request(operation),
        };
        if !route.validate() {
            return Err(CliError::Other("invalid remote mesh request".into()));
        }
        Ok(route)
    }

    async fn send(&self, operation: Operation) -> Result<WireResponse, CliError> {
        let route = self.route(operation)?;
        match output::check_response(
            client::send_request(self.socket, &Request::MeshRoute { route }).await?,
            self.json,
        )? {
            Response::MeshRoute { response } => Ok(response),
            _ => Err(CliError::Other("unexpected remote response".into())),
        }
    }

    /// Print the reply. A mutation's identity is part of the output so the
    /// exact operation can be retried: in JSON alongside the response,
    /// otherwise as a line on stderr.
    fn print(&self, response: WireResponse, issued: Option<Mutation>) -> Result<(), CliError> {
        let failed = matches!(response.body, Body::Error { .. });
        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "operation": issued,
                    "response": response,
                }))?
            );
        } else {
            if let Some(Mutation { window, id }) = &issued {
                eprintln!("operation: --window {window} --operation {id}");
            }
            output::print_response(&Response::MeshRoute { response }, false)?;
        }
        if failed {
            return Err(CliError::Other("worker refused the request".into()));
        }
        Ok(())
    }

    /// The run's current guard from durable status. Receipts are historical
    /// snapshots; control always uses the latest epoch and revision.
    async fn guard(&self, run: &RunSelector) -> Result<ControlGuard, CliError> {
        let export = Export {
            id: run.export.clone(),
            generation: run.export_generation,
        };
        let response = self
            .send(Operation::Status {
                export,
                agent_id: Some(run.agent.clone()),
                after_agent: None,
                limit: 1,
            })
            .await?;
        match response.body {
            Body::Status { agents, .. } => agents
                .into_iter()
                .next()
                .map(|agent| ControlGuard {
                    target: agent.target,
                    epoch: agent.epoch,
                    revision: agent.revision,
                })
                .ok_or_else(|| CliError::Other("no such remote agent".into())),
            body => Err(refused("the status request", &body)),
        }
    }

    /// Reuse the supplied identity, or issue a fresh window and operation ID.
    async fn mutation(&self, args: OperationArgs) -> Result<Mutation, CliError> {
        let window = match args.window {
            Some(window) => window,
            None => match self
                .send(Operation::IssueWindow {
                    ttl_seconds: DEFAULT_WINDOW_TTL,
                })
                .await?
                .body
            {
                Body::Window { window, .. } => window,
                body => return Err(refused("a window", &body)),
            },
        };
        let id = args
            .operation
            .unwrap_or_else(|| format!("op-{}", pu_core::id::session_id()));
        Ok(Mutation { window, id })
    }

    async fn watch(
        &self,
        run: RunSelector,
        cursor: Option<wire::OutputCursor>,
    ) -> Result<(), CliError> {
        let guard = self.guard(&run).await?;
        let route = self.route(Operation::SubscribeOutput {
            target: guard.target,
            cursor,
        })?;
        stream(self.socket, Request::MeshWatch { route }, self.json).await
    }

    /// The bridge operation for an action, plus the identity it was issued.
    async fn operation(
        &self,
        action: RemoteAction,
    ) -> Result<(Operation, Option<Mutation>), CliError> {
        Ok(match action {
            RemoteAction::Watch { .. } => {
                return Err(CliError::Other(
                    "watch streams output; it is not a one-shot operation".into(),
                ));
            }
            RemoteAction::Inventory => (Operation::Inventory {}, None),
            RemoteAction::Status {
                export,
                export_generation,
                agent,
                after,
                limit,
            } => (
                Operation::Status {
                    export: Export {
                        id: export,
                        generation: export_generation,
                    },
                    agent_id: agent,
                    after_agent: after,
                    limit,
                },
                None,
            ),
            RemoteAction::Window { ttl_seconds } => (Operation::IssueWindow { ttl_seconds }, None),
            RemoteAction::Spawn {
                export,
                export_generation,
                runtime,
                commit,
                prompt,
                operation,
            } => {
                let mutation = self.mutation(operation).await?;
                (
                    Operation::Spawn {
                        mutation: mutation.clone(),
                        export: Export {
                            id: export,
                            generation: export_generation,
                        },
                        runtime,
                        source_commit: commit,
                        prompt,
                    },
                    Some(mutation),
                )
            }
            RemoteAction::Acquire { run, operation } => {
                let guard = self.guard(&run).await?;
                let mutation = self.mutation(operation).await?;
                (
                    Operation::AcquireControl {
                        guard,
                        mutation: mutation.clone(),
                    },
                    Some(mutation),
                )
            }
            RemoteAction::Stop { run, operation } => {
                let guard = self.guard(&run).await?;
                let mutation = self.mutation(operation).await?;
                (
                    Operation::Stop {
                        guard,
                        mutation: mutation.clone(),
                    },
                    Some(mutation),
                )
            }
            RemoteAction::Resize {
                run,
                cols,
                rows,
                operation,
            } => {
                let guard = self.guard(&run).await?;
                let mutation = self.mutation(operation).await?;
                (
                    Operation::Resize {
                        guard,
                        mutation: mutation.clone(),
                        cols,
                        rows,
                    },
                    Some(mutation),
                )
            }
            RemoteAction::Writer {
                run,
                previous_writer,
                previous_sequence,
            } => (
                Operation::OpenWriter {
                    guard: self.guard(&run).await?,
                    previous: previous_writer.zip(previous_sequence).map(
                        |(writer, last_sequence)| WriterCheckpoint {
                            writer,
                            last_sequence,
                        },
                    ),
                },
                None,
            ),
            RemoteAction::Input {
                run,
                writer,
                sequence,
                text,
                submit,
            } => {
                let mut data = text.into_bytes();
                if submit {
                    data.push(b'\r');
                }
                (
                    Operation::Input {
                        guard: self.guard(&run).await?,
                        writer,
                        sequence,
                        data,
                    },
                    None,
                )
            }
            RemoteAction::InputReceipt {
                run,
                writer,
                sequence,
            } => (
                Operation::InputReceipt {
                    target: self.guard(&run).await?.target,
                    writer,
                    sequence,
                },
                None,
            ),
        })
    }
}

/// Owner-local stream of remote output frames until the run ends.
async fn stream(socket: &Path, request: Request, json: bool) -> Result<(), CliError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let stream = tokio::net::UnixStream::connect(socket)
        .await
        .map_err(|_| CliError::DaemonNotRunning)?;
    let (reader, mut writer) = stream.into_split();
    let mut line = serde_json::to_vec(&request)?;
    line.push(b'\n');
    writer.write_all(&line).await?;
    let mut reader = BufReader::new(reader);
    let mut text = String::new();
    let mut cursor: Option<wire::OutputCursor> = None;
    loop {
        text.clear();
        if reader.read_line(&mut text).await? == 0 {
            let resume = cursor
                .map(|c| format!("; resume with --cursor {}:{}", c.incarnation, c.offset))
                .unwrap_or_default();
            return Err(CliError::Other(format!(
                "watch ended before the run's output ended{resume}"
            )));
        }
        let response: Response = serde_json::from_str(text.trim())?;
        match output::check_response(response, json)? {
            Response::MeshOutput { frame } => {
                if let Some(gap) = frame.gap {
                    eprintln!("[mesh] {gap} output bytes were evicted before delivery");
                }
                cursor = Some(wire::OutputCursor {
                    incarnation: frame.incarnation.clone(),
                    offset: frame.offset + frame.data.len() as u64,
                });
                let end = frame.end;
                output::print_response(&Response::MeshOutput { frame }, json)?;
                if end {
                    eprintln!("[mesh] run output ended");
                    return Ok(());
                }
            }
            Response::MeshRoute { response } => {
                if let Body::Error { code } = &response.body {
                    let message = format!("worker refused the subscription: {code:?}");
                    output::print_response(&Response::MeshRoute { response }, json)?;
                    return Err(CliError::Other(message));
                }
            }
            other => output::print_response(&other, json)?,
        }
    }
}

pub async fn run(
    socket: &Path,
    selector: &str,
    action: RemoteAction,
    json: bool,
) -> Result<(), CliError> {
    let workers = match output::check_response(
        client::send_request(
            socket,
            &Request::MeshAdmin {
                request: MeshAdminRequest::WorkerList {},
            },
        )
        .await?,
        json,
    )? {
        Response::MeshAdmin {
            response: MeshAdminResponse::Workers { workers },
        } => workers,
        _ => return Err(CliError::Other("unexpected worker list response".into())),
    };
    let remote = Remote {
        socket,
        worker: select_worker(workers, selector)?,
        json,
    };
    if let RemoteAction::Watch { run, cursor } = action {
        return remote.watch(run, cursor).await;
    }
    let (operation, issued) = remote.operation(action).await?;
    let response = remote.send(operation).await?;
    remote.print(response, issued)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(fingerprint: char, label: &str) -> WorkerRoute {
        WorkerRoute {
            fingerprint: fingerprint.to_string().repeat(64),
            daemon_id: "worker".into(),
            label: label.into(),
            control_address: "192.168.1.20:7443".parse().unwrap(),
            bulk_address: "192.168.1.20:7444".parse().unwrap(),
        }
    }

    #[test]
    fn given_fingerprint_or_unique_label_should_select_exactly_one_worker() {
        // given
        let workers = vec![route('a', "studio"), route('b', "laptop")];
        // when / then
        assert_eq!(
            select_worker(workers.clone(), "studio").unwrap().label,
            "studio"
        );
        assert_eq!(
            select_worker(workers.clone(), &"b".repeat(64))
                .unwrap()
                .label,
            "laptop"
        );
        assert!(select_worker(workers, "missing").is_err());
    }

    #[test]
    fn given_ambiguous_label_should_require_a_fingerprint() {
        // given
        let workers = vec![route('a', "studio"), route('b', "studio")];
        // when / then
        assert!(select_worker(workers, "studio").is_err());
    }

    #[test]
    fn given_cursor_text_should_parse_incarnation_and_offset() {
        // given / when
        let cursor = parse_cursor("abc123:4096").unwrap();
        // then
        assert_eq!(cursor.incarnation, "abc123");
        assert_eq!(cursor.offset, 4096);
        assert!(parse_cursor("no-offset").is_err());
        assert!(parse_cursor("abc:x").is_err());
    }
}
