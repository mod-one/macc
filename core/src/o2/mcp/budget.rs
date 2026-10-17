//! Bound the complete JSON-RPC tool response, including escaped content and request ID.
use super::*;
pub(super) fn response(id: &Value, data: &Value, is_error: bool) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":data.to_string()}],"isError":is_error}})
}
fn fits(value: &Value, limit: u64) -> bool {
    crate::context::payload::json_tokens(value).is_ok_and(|n| n as u64 <= limit)
}
pub(super) fn bound(id: &Value, mut data: Value, is_error: bool, limit: u64) -> Value {
    loop {
        let wire = response(id, &data, is_error);
        if fits(&wire, limit) {
            return wire;
        }
        if let Some(rows) = data["records"].as_array_mut() {
            if rows.len() > 1 {
                rows.pop();
                let count = rows.len();
                data["next_cursor"] = json!(data["cursor"].as_u64().unwrap_or(0) + count as u64);
                data["transport_truncated"] = json!(true);
                continue;
            }
            if let Some(row) = rows.pop() {
                data["references"] = json!([{"id":row["memory"]["id"],"checksum":row["checksum"]}]);
                data["next_cursor"] = json!(data["cursor"].as_u64().unwrap_or(0) + 1);
                data["notice"] =
                    json!("Use memory.get for this record; continue search with next_cursor");
                continue;
            }
        }
        if let Some(fragment) = data["fragment"].as_str() {
            let count = fragment.chars().count();
            if count > 1 {
                data["fragment"] = json!(fragment.chars().take(count / 2).collect::<String>());
                data["next_offset"] =
                    json!(data["offset"].as_u64().unwrap_or(0) + (count / 2) as u64);
                continue;
            }
        }
        if let Some(hits) = data["data"]["hits"].as_array_mut() {
            if hits.pop().is_some() {
                data["transport_truncated"] = json!(true);
                continue;
            }
        }
        return response(
            id,
            &json!({"error":"Output budget exceeded; narrow the query or request a smaller fragment"}),
            true,
        );
    }
}
