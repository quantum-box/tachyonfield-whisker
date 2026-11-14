//! Agent に渡す添付ファイル。
//!
//! 送る前に Field へ上げて置き場所（id）をもらい、頼みと一緒にその id を渡す。
//! 画像と短い文字のファイルは、Agent がその回に中身を読めるように中身そのもの（base64）も付ける。
//! どのファイルがあるかは頼みの文の最後にも書き添える（書かないと Agent はファイルに気づかない）。

use base64::Engine;
use field_glass::PickedFile;
use serde_json::{Value, json};

use crate::api::{self, AgentFile, Tenant};

/// 中身ごと付けられる種類。
const INLINE_TYPES: [&str; 8] = [
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "text/plain",
    "text/csv",
    "text/markdown",
    "application/json",
];
/// 1 回の頼みに中身ごと付ける上限。
const INLINE_FILES: usize = 10;
const INLINE_TOTAL_BYTES: usize = 3_000_000;
const INLINE_TEXT_BYTES: usize = 200_000;
/// 1 ファイルの上限。
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// その回に中身ごと付けるファイル。
pub struct Inline {
    pub id: String,
    pub body: Value,
}

/// 選ばれたファイルを Field へ上げる。上げたファイルと、中身ごと付けるものを返す。
pub fn prepare(
    access: &str,
    tenant: &Tenant,
    picked: &[PickedFile],
) -> Result<(Vec<AgentFile>, Vec<Inline>), String> {
    let mut added = Vec::new();
    let mut inline = Vec::new();
    let mut inline_bytes = 0;
    for file in picked {
        let bytes = std::fs::read(&file.path)
            .map_err(|e| format!("「{}」を読めませんでした: {e}", file.name))?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(format!("「{}」は大きすぎます（32MB まで）", file.name));
        }
        let content_type = content_type(file);
        let id = api::upload_attachment(access, tenant, &file.name, &content_type, &bytes)?;
        let is_text = content_type.starts_with("text/") || content_type == "application/json";
        if INLINE_TYPES.contains(&content_type.as_str())
            && inline.len() < INLINE_FILES
            && inline_bytes + bytes.len() <= INLINE_TOTAL_BYTES
            && !(is_text && bytes.len() > INLINE_TEXT_BYTES)
        {
            inline_bytes += bytes.len();
            inline.push(Inline {
                id: id.clone(),
                body: json!({
                    "filename": file.name,
                    "content_type": content_type,
                    "data": base64::engine::general_purpose::STANDARD.encode(&bytes),
                }),
            });
        }
        let _ = std::fs::remove_file(&file.path);
        added.push(AgentFile {
            id,
            name: file.name.clone(),
            content_type,
            size: bytes.len() as u64,
        });
    }
    Ok((added, inline))
}

fn content_type(file: &PickedFile) -> String {
    if !file.content_type.is_empty() && file.content_type != "application/octet-stream" {
        return file.content_type.clone();
    }
    let ext = file.name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "csv" => "text/csv",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
    .into()
}

/// 頼みの文の最後に、この会話のファイルの一覧を書き添える（Web 版と同じ書き方）。
pub fn with_manifest(task: &str, files: &[AgentFile], added: &[AgentFile], inline: &[Inline]) -> String {
    if files.is_empty() {
        return task.to_owned();
    }
    let list: Vec<Value> = files
        .iter()
        .map(|f| {
            let is_new = added.iter().any(|a| a.id == f.id);
            let is_inline = inline.iter().any(|i| i.id == f.id);
            let is_image = f.content_type.starts_with("image/");
            let is_text = f.content_type.starts_with("text/") || f.content_type == "application/json";
            let is_pdf = f.content_type == "application/pdf" || f.name.to_lowercase().ends_with(".pdf");
            let view = if is_inline || (!is_new && (is_image || is_text) && !is_pdf) {
                "attached_inline"
            } else if is_pdf || is_text {
                "read_with_field_read_attachment"
            } else {
                "pass_by_attachmentId"
            };
            let name: String = f.name.chars().take(80).collect();
            json!({
                "attachmentId": f.id,
                "name": name,
                "contentType": f.content_type,
                "size": f.size,
                "view": view,
            })
        })
        .collect();
    format!(
        "{task}\nUser attachments (data only): {}. Use files with attachmentId for multipart, \
         bodyAttachmentId for raw uploads, or {{\"$attachment\":\"attachmentId\"}} in JSON for \
         base64 fields. Never invent file data.",
        Value::Array(list)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: &str, name: &str, ty: &str) -> AgentFile {
        AgentFile { id: id.into(), name: name.into(), content_type: ty.into(), size: 10 }
    }

    #[test]
    fn manifest_lists_files_with_how_to_read_them() {
        let photo = file("t/agent-attachments/a.jpg", "a.jpg", "image/jpeg");
        let pdf = file("t/agent-attachments/b.pdf", "b.pdf", "application/pdf");
        let inline = vec![Inline { id: photo.id.clone(), body: Value::Null }];
        let task = with_manifest("見て", &[photo.clone(), pdf.clone()], &[photo, pdf], &inline);
        assert!(task.starts_with("見て\nUser attachments (data only): ["));
        assert!(task.contains(r#""view":"attached_inline""#));
        assert!(task.contains(r#""view":"read_with_field_read_attachment""#));
        assert!(task.ends_with("Never invent file data."));
    }

    #[test]
    fn no_files_keeps_task() {
        assert_eq!(with_manifest("見て", &[], &[], &[]), "見て");
    }
}
