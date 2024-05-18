use nu_engine::command_prelude::*;
use nu_protocol::HistoryFileFormat;
use reedline::{
    FileBackedHistory, History as ReedlineHistory, HistoryItem, SearchDirection, SearchQuery,
    SqliteBackedHistory, RqliteBackedHistory, HistoryStorageDest,
    ReedlineError, ReedlineErrorVariants,
};
use nu_protocol::{HISTORY_DEST_TXT, HISTORY_DEST_SQLITE};

#[derive(Clone)]
pub struct History;

impl Command for History {
    fn name(&self) -> &str {
        "history"
    }

    fn usage(&self) -> &str {
        "Get the command history."
    }

    fn signature(&self) -> nu_protocol::Signature {
        Signature::build("history")
            .input_output_types(vec![(Type::Nothing, Type::Any)])
            .allow_variants_without_examples(true)
            .switch("clear", "Clears out the history entries", Some('c'))
            .switch(
                "long",
                "Show long listing of entries for sqlite history",
                Some('l'),
            )
            .category(Category::History)
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let head = call.head;

        let Some(history) = engine_state.history_config() else {
            return Ok(PipelineData::empty());
        };

        // todo for sqlite history this command should be an alias to `open ~/.config/nushell/history.sqlite3 | get history`
        match nu_path::config_dir() {
            None => Err(ShellError::ConfigDirNotFound { span: Some(head) }),
            Some(config_path) => {
                let clear = call.has_flag(engine_state, stack, "clear")?;
                let long = call.has_flag(engine_state, stack, "long")?;
                let ctrlc = engine_state.ctrlc.clone();

                let history_dest = match history.file_format {
                    | HistoryFileFormat::Sqlite
                    | HistoryFileFormat::PlainText
                    => {
                        let mut history_path = config_path;
                        history_path.push("nushell");
                        if matches!(history.file_format, HistoryFileFormat::Sqlite)
                        {
                            history_path.push(HISTORY_DEST_SQLITE);
                        } else {
                            history_path.push(HISTORY_DEST_TXT);
                        }

                        HistoryStorageDest::Path(history_path)
                    }
                    HistoryFileFormat::Rqlite => history.rqlite_url.into(),
                };

                if clear {
                    if let HistoryStorageDest::Path(history_path) = history_dest {
                        let _ = std::fs::remove_file(history_path);
                        // TODO: FIXME also clear the auxiliary files when using sqlite
                    }
                    return Ok(PipelineData::empty());
                }
                let history_reader: Box<dyn ReedlineHistory> = match history.file_format {
                    HistoryFileFormat::Sqlite => SqliteBackedHistory::with_file(history_dest.clone(), None, None)
                        .map(|inner| {
                            let boxed: Box<dyn ReedlineHistory> = Box::new(inner);
                            boxed
                        })
                        .map_err(map_shell_io_error(history_dest.clone())),
                    HistoryFileFormat::PlainText => FileBackedHistory::with_file(history.max_size as usize, history_dest.clone())
                        .map(|inner| {
                            let boxed: Box<dyn ReedlineHistory> = Box::new(inner);
                            boxed
                        })
                        .map_err(map_shell_io_error(history_dest.clone())),
                    HistoryFileFormat::Rqlite => RqliteBackedHistory::with_url(history_dest.clone(), None, None)
                        .map(|inner| {
                            let boxed: Box<dyn ReedlineHistory> = Box::new(inner);
                            boxed
                        })
                        .map_err(|err| ShellError::NetworkFailure {
                            msg: format!("Failed to connect rqlite: {history_dest}\n{err:?}"),
                            span: head,
                        }),
                }?;

                match history.file_format {
                    HistoryFileFormat::PlainText => Ok(history_reader
                        .search(SearchQuery::everything(SearchDirection::Forward, None))
                        .map(move |entries| {
                            entries.into_iter().enumerate().map(move |(idx, entry)| {
                                Value::record(
                                    record! {
                                        "command" => Value::string(entry.command_line, head),
                                        "index" => Value::int(idx as i64, head),
                                    },
                                    head,
                                )
                            })
                        })
                        .map_err(|_| ShellError::FileNotFound {
                            file: history_dest.to_string(),
                            span: head,
                        })?
                        .into_pipeline_data(head, ctrlc)),
                    HistoryFileFormat::Sqlite => Ok(history_reader
                        .search(SearchQuery::everything(SearchDirection::Forward, None))
                        .map(move |entries| {
                            entries.into_iter().enumerate().map(move |(idx, entry)| {
                                create_history_record(idx, entry, long, head)
                            })
                        })
                        .map_err(|_| ShellError::FileNotFound {
                            file: history_dest.to_string(),
                            span: head,
                        })?
                        .into_pipeline_data(head, ctrlc)),
                    HistoryFileFormat::Rqlite => Ok(history_reader
                        .search(SearchQuery::everything(SearchDirection::Forward, None))
                        .map(move |entries|
                            entries.into_iter().enumerate().map(move |(idx, entry)|
                                create_history_record(idx, entry, long, head)
                            )
                        )
                        .map_err(|err| ShellError::NetworkFailure {
                            msg: if let ReedlineError(ReedlineErrorVariants::HistoryDatabaseError(msg)) = err {
                                format!("Failed to connect rqlite: {history_dest}\n{msg}")
                            } else {
                                format!("Failed to connect rqlite: {history_dest}")
                            },
                            span: head,
                        })?
                        .into_pipeline_data(head, ctrlc)
                    ),
                }
            }
        }
    }

    fn examples(&self) -> Vec<Example> {
        vec![
            Example {
                example: "history | length",
                description: "Get current history length",
                result: None,
            },
            Example {
                example: "history | last 5",
                description: "Show last 5 commands you have ran",
                result: None,
            },
            Example {
                example: "history | where command =~ cargo | get command",
                description: "Search all the commands from history that contains 'cargo'",
                result: None,
            },
        ]
    }
}

fn map_shell_io_error(dest: HistoryStorageDest) -> impl Fn(ReedlineError) -> ShellError {
    move |err| {
        ShellError::IOError {
            msg: format!("{}, {:?}", dest, err),
        }
    }
}

fn create_history_record(idx: usize, entry: HistoryItem, long: bool, head: Span) -> Value {
    //1. Format all the values
    //2. Create a record of either short or long columns and values

    let item_id_value = Value::int(
        match entry.id {
            Some(id) => {
                let ids = id.to_string();
                match ids.parse::<i64>() {
                    Ok(i) => i,
                    _ => 0i64,
                }
            }
            None => 0i64,
        },
        head,
    );
    let start_timestamp_value = Value::string(
        match entry.start_timestamp {
            Some(time) => time.to_string(),
            None => "".into(),
        },
        head,
    );
    let command_value = Value::string(entry.command_line, head);
    let session_id_value = Value::int(
        match entry.session_id {
            Some(sid) => {
                let sids = sid.to_string();
                match sids.parse::<i64>() {
                    Ok(i) => i,
                    _ => 0i64,
                }
            }
            None => 0i64,
        },
        head,
    );
    let hostname_value = Value::string(
        match entry.hostname {
            Some(host) => host,
            None => "".into(),
        },
        head,
    );
    let cwd_value = Value::string(
        match entry.cwd {
            Some(cwd) => cwd,
            None => "".into(),
        },
        head,
    );
    let duration_value = Value::duration(
        match entry.duration {
            Some(d) => d.as_nanos().try_into().unwrap_or(0),
            None => 0,
        },
        head,
    );
    let exit_status_value = Value::int(entry.exit_status.unwrap_or(0), head);
    let index_value = Value::int(idx as i64, head);
    if long {
        Value::record(
            record! {
                "item_id" => item_id_value,
                "start_timestamp" => start_timestamp_value,
                "command" => command_value,
                "session_id" => session_id_value,
                "hostname" => hostname_value,
                "cwd" => cwd_value,
                "duration" => duration_value,
                "exit_status" => exit_status_value,
                "idx" => index_value,
            },
            head,
        )
    } else {
        Value::record(
            record! {
                "start_timestamp" => start_timestamp_value,
                "command" => command_value,
                "cwd" => cwd_value,
                "duration" => duration_value,
                "exit_status" => exit_status_value,
            },
            head,
        )
    }
}
