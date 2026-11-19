//! Native receipt attachments shared by editable business forms.
use super::model;
use crate::cash_book_screen::spawn_in;
use crate::*;
use serde_json::json;
use whisker::Owner;

const MAX_BYTES: u64 = 32 * 1024 * 1024;

pub(crate) fn content_type(file: &field_glass::PickedFile) -> Result<&'static str, String> {
    match file.content_type.as_str() {
        "image/jpeg" => Ok("image/jpeg"),
        "image/png" => Ok("image/png"),
        "image/heic" => Ok("image/heic"),
        "image/heif" => Ok("image/heif"),
        "application/pdf" => Ok("application/pdf"),
        "" | "application/octet-stream" => match file
            .name
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "jpg" | "jpeg" => Ok("image/jpeg"),
            "png" => Ok("image/png"),
            "heic" => Ok("image/heic"),
            "heif" => Ok("image/heif"),
            "pdf" => Ok("application/pdf"),
            _ => Err("写真（JPEG・PNG・HEIC）またはPDFを選んでください".into()),
        },
        _ => Err("写真（JPEG・PNG・HEIC）またはPDFを選んでください".into()),
    }
}

pub(crate) fn read(file: &field_glass::PickedFile, max_bytes: u64) -> Result<Vec<u8>, String> {
    let size = std::fs::metadata(&file.path)
        .map_err(|_| "選んだファイルを読み込めませんでした")?
        .len();
    if size == 0 || size > max_bytes {
        return Err(format!(
            "ファイルは1バイト以上、{}MB以下にしてください",
            max_bytes / 1024 / 1024
        ));
    }
    let bytes = std::fs::read(&file.path).map_err(|_| "選んだファイルを読み込めませんでした")?;
    if bytes.is_empty() || bytes.len() as u64 > max_bytes {
        return Err("ファイルのサイズが変わりました。選び直してください".into());
    }
    Ok(bytes)
}

#[component]
pub(crate) fn receipt_attachments(
    label: &'static str,
    kind: &'static str,
    draft: RwSignal<Value>,
    path: String,
    token: RwSignal<Option<String>>,
    tenant: RwSignal<Option<Tenant>>,
) -> Element {
    let path = signal(path.clone());
    let busy = signal(false);
    let error = signal(String::new());
    let owner = Owner::current();
    let upload = move |file: field_glass::PickedFile| {
        if busy.get_untracked() {
            return;
        }
        let Some(current) = tenant.get_untracked() else {
            return;
        };
        let context = current.clone();
        let ty = match content_type(&file) {
            Ok(ty) => ty,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        busy.set(true);
        error.set(String::new());
        draft.update(|v| {
            v["_nativeUploadsInFlight"] =
                json!(v["_nativeUploadsInFlight"].as_u64().unwrap_or(0) + 1)
        });
        let name = if file.name.is_empty() {
            "添付資料".to_owned()
        } else {
            file.name.clone()
        };
        spawn_in(owner, async move {
            let name_to_send = name.clone();
            let result = authed(token, move |access| {
                let bytes = read(&file, MAX_BYTES)?;
                let response =
                    api::upload_receipt(access, &current, &name_to_send, ty, &bytes, kind, None)?;
                let id = response["id"]
                    .as_str()
                    .or_else(|| response["receipt"]["id"].as_str())
                    .filter(|id| !id.is_empty())
                    .ok_or("添付資料の保存結果を確認できませんでした")?
                    .to_owned();
                let _ = std::fs::remove_file(&file.path);
                Ok(id)
            })
            .await;
            if !owner.is_none_or(Owner::is_alive)
                || tenant.get_untracked().as_ref() != Some(&context)
                || token.with_untracked(Option::is_none)
            {
                return;
            }
            busy.set(false);
            draft.update(|v| {
                v["_nativeUploadsInFlight"] = json!(
                    v["_nativeUploadsInFlight"]
                        .as_u64()
                        .unwrap_or(0)
                        .saturating_sub(1)
                )
            });
            match result {
                Ok(id) => draft.update(|v| {
                    let mut files = model::value_at(v, &path.get_untracked())
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    if !files
                        .iter()
                        .any(|file| file["receiptId"].as_str() == Some(id.as_str()))
                    {
                        files.push(json!({"receiptId":id,"fileName":name}));
                    }
                    model::set_at(v, &path.get_untracked(), Value::Array(files));
                }),
                Err(message) => error.set(message),
            }
        });
    };
    render! {
        View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Column, padding: px(16), gap: px(8))) {
            Text(value: label, style: computed(move || css!(font_size: px(16), color: hex(LABEL))))
            ForEach(
                each: move || draft.with(|v| model::value_at(v, &path.get()).as_array().cloned().unwrap_or_default()),
                key: |file: &Value| model::input_text(&file["receiptId"]),
                children: move |file: ReadSignal<Value>| render! {
                    View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Row, align_items: AlignItems::Center, gap: px(8))) {
                        Text(value: computed(move || file.with(|file| file["fileName"].as_str().unwrap_or("添付資料").to_owned())), style: computed(move || css!(font_size: px(15), color: hex(LABEL), flex_grow: 1.0)))
                        Show(when: move || !busy.get()) {
                            Text(value: "外す", style: computed(move || css!(padding: px(8), color: hex(RED), font_size: px(15))), on_tap: move |_| {
                                let id = file.get_untracked()["receiptId"].clone();
                                draft.update(|v| {
                                    let files: Vec<_> = model::value_at(v, &path.get_untracked()).as_array().into_iter().flatten().filter(|f| f["receiptId"] != id).cloned().collect();
                                    model::set_at(v, &path.get_untracked(), Value::Array(files));
                                });
                            })
                        }
                    }
                },
            )
            Show(when: move || !busy.get()) {
                View(style: css!(display: Display::Flex, flex_direction: FlexDirection::Row, align_items: AlignItems::Center, gap: px(8))) {
                    field_glass::AttachButton(on_pick: move |file: field_glass::PickedFile| upload(file), style: css!(width: px(44), height: px(44)))
                    Text(value: if cfg!(target_os = "ios") { "写真・PDFを添付" } else { "写真・PDFの添付はiOSで利用できます" }, style: computed(move || css!(font_size: px(14), color: hex(SECONDARY_LABEL))))
                }
            }
            Show(when: move || busy.get()) { Notice(message: "添付資料を送信中…") }
            Show(when: move || !error.with(String::is_empty)) { ErrorNotice(message: computed(move || error.get())) }
        }
    }
}
