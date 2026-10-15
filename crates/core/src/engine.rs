//! The session every front end drives: one model, its undo history and the command registry.
//! Every user-visible action is a [`Command`]; the CLI, scripts and MCP all reach the same ones.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::model::Model;

pub type Run = fn(&mut Session, &Value) -> Result<Value, String>;

pub struct Command {
    /// Lower-case id, after Revit's tool name.
    pub id: &'static str,
    pub label: &'static str,
    /// Typed shortcuts, Revit style (WA, DR, WN…).
    pub aliases: &'static [&'static str],
    /// The JSON parameters, documented for agents.
    pub params: &'static str,
    pub run: Run,
    /// Whether a change it makes is an undo step.
    pub undoable: bool,
}

const UNDO_LIMIT: usize = 200;

pub struct Session {
    pub model: Arc<Model>,
    /// Where the model was opened from or last saved to.
    pub path: Option<PathBuf>,
    undo: Vec<(&'static str, Arc<Model>)>,
    redo: Vec<(&'static str, Arc<Model>)>,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

pub fn commands() -> &'static [Command] {
    crate::commands::ALL
}

/// A command by id or alias, in any case.
pub fn find(name: &str) -> Option<&'static Command> {
    commands().iter().find(|c| c.id.eq_ignore_ascii_case(name) || c.aliases.iter().any(|a| a.eq_ignore_ascii_case(name)))
}

/// The command catalog as agents read it.
pub fn catalog() -> Value {
    commands().iter().map(|c| json!({"id": c.id, "label": c.label, "aliases": c.aliases, "params": c.params})).collect()
}

impl Session {
    pub fn new() -> Session {
        Session { model: Arc::new(Model::new("Project")), path: None, undo: Vec::new(), redo: Vec::new() }
    }

    /// The model to change, copied first when an undo step still shares it.
    pub fn model_mut(&mut self) -> &mut Model {
        Arc::make_mut(&mut self.model)
    }

    /// Replace the model (new, open), forgetting the history.
    pub fn reset(&mut self, model: Model, path: Option<PathBuf>) {
        self.model = Arc::new(model);
        self.path = path;
        self.undo.clear();
        self.redo.clear();
    }

    /// Labels of the undo steps, oldest first.
    pub fn history(&self) -> Vec<&'static str> {
        self.undo.iter().map(|(l, _)| *l).collect()
    }

    /// Run a command by id or alias with JSON parameters. A command that fails, panics, or leaves
    /// the model failing [`Model::check`] changes nothing.
    pub fn execute(&mut self, name: &str, params: &Value) -> Result<Value, String> {
        let cmd = find(name).ok_or_else(|| format!("unknown command `{name}`; list_commands shows them"))?;
        let before = Arc::clone(&self.model);
        let run = cmd.run;
        let result = catch_unwind(AssertUnwindSafe(|| run(self, params)))
            .unwrap_or_else(|p| Err(format!("internal error in `{}`: {}", cmd.id, panic_text(p.as_ref()))))
            .and_then(|v| if Arc::ptr_eq(&before, &self.model) { Ok(v) } else { self.model.check().map(|()| v) });
        match result {
            Err(e) => {
                self.model = before;
                Err(e)
            }
            Ok(v) => {
                if cmd.undoable && !Arc::ptr_eq(&before, &self.model) {
                    self.undo.push((cmd.label, before));
                    if self.undo.len() > UNDO_LIMIT {
                        self.undo.remove(0);
                    }
                    self.redo.clear();
                }
                Ok(v)
            }
        }
    }

    /// Run a script: one command per line, `id` or `id {json params}`; `#` starts a comment.
    /// Stops at the first failing line, naming it; the lines before it stay done.
    pub fn script(&mut self, text: &str) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (id, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            let params = if rest.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(rest).map_err(|e| format!("line {}: bad JSON after `{id}`: {e}", n + 1))?
            };
            out.push(self.execute(id, &params).map_err(|e| format!("line {} ({id}): {e}", n + 1))?);
        }
        Ok(out)
    }

    pub(crate) fn step_back(&mut self, redo: bool) -> Result<Value, String> {
        let (from, to) = if redo { (&mut self.redo, &mut self.undo) } else { (&mut self.undo, &mut self.redo) };
        let (label, model) = from.pop().ok_or(if redo { "nothing to redo" } else { "nothing to undo" })?;
        to.push((label, std::mem::replace(&mut self.model, model)));
        let key = if redo { "redone" } else { "undone" };
        Ok(json!({ key: label }))
    }
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into())
}
