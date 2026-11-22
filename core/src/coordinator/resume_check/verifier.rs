//! A supervisor program evaluates a fresh observation using the bundled SQLite VM.
//! The connection is private to this invocation. It has no project database or host functions.
use super::{error, Outcome};
use crate::Result;
use rusqlite::{
    hooks::{AuthAction, AuthContext, Authorization},
    limits::Limit,
    Batch, Connection,
};
use std::time::Instant;

pub(super) fn evaluate(
    script: &str,
    input: &serde_json::Value,
    deadline: Instant,
) -> Result<Outcome> {
    let run = || -> rusqlite::Result<String> {
        let db = Connection::open_in_memory()?;
        db.execute_batch("PRAGMA temp_store=MEMORY; PRAGMA query_only=ON; SELECT value FROM json_each('[]'); SELECT value FROM json_tree('[]')")?;
        db.set_limit(Limit::SQLITE_LIMIT_LENGTH, 512 * 1024);
        db.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, 16384);
        db.set_limit(Limit::SQLITE_LIMIT_EXPR_DEPTH, 40);
        db.set_limit(Limit::SQLITE_LIMIT_COLUMN, 32);
        db.set_limit(Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH, 256);
        db.set_limit(Limit::SQLITE_LIMIT_COMPOUND_SELECT, 8);
        db.set_limit(Limit::SQLITE_LIMIT_VDBE_OP, 8192);
        db.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 1);
        let mut ticks = 0;
        db.progress_handler(
            100,
            Some(move || {
                ticks += 1;
                ticks > 1000 || Instant::now() >= deadline
            }),
        );
        db.authorizer(Some(|ctx: AuthContext<'_>| match ctx.action {
            AuthAction::Select | AuthAction::Recursive => Authorization::Allow,
            AuthAction::Read {
                table_name: "json_each" | "json_tree",
                ..
            } => Authorization::Allow,
            AuthAction::Function { function_name }
                if function_name.starts_with("json")
                    || matches!(
                        function_name,
                        "->" | "->>"
                            | "typeof"
                            | "length"
                            | "lower"
                            | "upper"
                            | "trim"
                            | "instr"
                            | "substr"
                            | "coalesce"
                            | "ifnull"
                            | "nullif"
                            | "iif"
                            | "like"
                            | "glob"
                            | "count"
                            | "sum"
                            | "min"
                            | "max"
                            | "round"
                            | "abs"
                            | "unixepoch"
                            | "datetime"
                            | "strftime"
                            | "julianday"
                    ) =>
            {
                Authorization::Allow
            }
            _ => Authorization::Deny,
        }));
        let mut batch = Batch::new(&db, script);
        let mut stmt = batch.next()?.ok_or(rusqlite::Error::InvalidQuery)?;
        if batch.next()?.is_some()
            || !stmt.readonly()
            || stmt.column_count() != 1
            || stmt.parameter_count() != 1
            || stmt.parameter_name(1) != Some(":input")
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut rows = stmt.query([input.to_string()])?;
        let result: String = rows
            .next()?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?
            .get(0)?;
        if rows.next()?.is_some() || result.len() > 4096 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Ok(result)
    };
    let result = run().map_err(|e| error(format!("Verifier query failed: {e}")))?;
    let outcome: Outcome = serde_json::from_str(&result)
        .map_err(|e| error(format!("Invalid verifier result: {e}")))?;
    if Instant::now() >= deadline
        || outcome.detail.trim().is_empty()
        || outcome.detail.len() > 1024
        || outcome.detail.chars().any(char::is_control)
    {
        return Err(error(
            "Verifier exceeded its deadline or returned an invalid detail",
        ));
    }
    Ok(outcome)
}
