//! Reservation reports are server-generated CSV, not a dump of the visible page.

use super::{Tenant, describe_error, encode_query, field_request};
use chrono::NaiveDate;
use std::io::Read;
use std::time::Duration;

const EXPORT_PATH: &str = "/v1/erp/reservations/export.csv";
const MAX_CSV_BYTES: usize = 20 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReservationCsvExport {
    pub filename: String,
    /// Preserve UTF-8, CSV quoting, line endings and the server's formula protection.
    pub text: String,
}

fn report_date(value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| "CSVの期間はYYYY-MM-DDで指定してください".to_owned())?;
    if value.len() != 10 || parsed.format("%Y-%m-%d").to_string() != value {
        return Err("CSVの期間はYYYY-MM-DDで指定してください".into());
    }
    Ok(Some(value.to_owned()))
}

fn export_path(from: Option<&str>, to: Option<&str>) -> Result<(String, String), String> {
    let from = report_date(from)?;
    let to = report_date(to)?;
    if matches!((&from, &to), (Some(from), Some(to)) if from > to) {
        return Err("CSVの終了日は開始日以降にしてください".into());
    }
    let mut query = Vec::new();
    if let Some(from) = &from {
        query.push(format!("from={}", encode_query(from)));
    }
    if let Some(to) = &to {
        query.push(format!("to={}", encode_query(to)));
    }
    let path = if query.is_empty() {
        EXPORT_PATH.into()
    } else {
        format!("{EXPORT_PATH}?{}", query.join("&"))
    };
    let filename = match (from, to) {
        (Some(from), Some(to)) => format!("reservations_{from}_{to}.csv"),
        (Some(from), None) => format!("reservations_from_{from}.csv"),
        (None, Some(to)) => format!("reservations_to_{to}.csv"),
        _ => "reservations.csv".into(),
    };
    Ok((path, filename))
}

fn response_filename(disposition: Option<&str>, fallback: &str) -> String {
    disposition
        .and_then(|value| {
            value.split(';').find_map(|part| {
                let (key, value) = part.trim().split_once('=')?;
                if !key.eq_ignore_ascii_case("filename") {
                    return None;
                }
                let name = value.trim().trim_matches('"');
                (name.starts_with("reservations_")
                    && name.ends_with(".csv")
                    && name.len() <= 128
                    && name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.')))
                .then(|| name.to_owned())
            })
        })
        .unwrap_or_else(|| fallback.to_owned())
}

fn csv_text(bytes: Vec<u8>, content_type: &str) -> Result<String, String> {
    if !content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/csv"))
    {
        return Err("CSV形式の応答を取得できませんでした。もう一度読み込んでください".into());
    }
    if bytes.len() > MAX_CSV_BYTES {
        return Err("CSVが20MBを超えています。期間を短くして、もう一度取得してください".into());
    }
    let text =
        String::from_utf8(bytes).map_err(|_| "CSVの文字コードを読み取れませんでした".to_owned())?;
    if !text
        .trim_start_matches('\u{feff}')
        .starts_with("reservation_id,reservation_number,reservation_type_id,")
    {
        return Err("予約CSVの内容を確認できませんでした。もう一度読み込んでください".into());
    }
    Ok(text)
}

/// Fetch only after an explicit export action. Dates use the report API's day
/// filter; list search/status/type/resource filters are not supported by it.
pub fn export_reservations_csv(
    token: &str,
    tenant: &Tenant,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<ReservationCsvExport, String> {
    let (path, fallback) = export_path(from, to)?;
    let response = field_request("GET", &path, token, &tenant.id, &tenant.platform_id)
        .set("Accept", "text/csv")
        .timeout(Duration::from_secs(45))
        .call()
        .map_err(describe_error)?;
    let content_type = response
        .header("Content-Type")
        .unwrap_or_default()
        .to_owned();
    let filename = response_filename(response.header("Content-Disposition"), &fallback);
    if response
        .header("Content-Length")
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|size| size > MAX_CSV_BYTES)
    {
        return Err("CSVが20MBを超えています。期間を短くして、もう一度取得してください".into());
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take((MAX_CSV_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "CSVの受信が完了しませんでした。もう一度取得してください".to_owned())?;
    Ok(ReservationCsvExport {
        filename,
        text: csv_text(bytes, &content_type)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_uses_date_filters_and_never_list_search_filters() {
        assert_eq!(
            export_path(None, None).unwrap(),
            (EXPORT_PATH.into(), "reservations.csv".into())
        );
        assert_eq!(
            export_path(Some("2026-11-01"), Some("2026-11-30")).unwrap(),
            (
                format!("{EXPORT_PATH}?from=2026-11-01&to=2026-11-30"),
                "reservations_2026-11-01_2026-11-30.csv".into()
            )
        );
        assert_eq!(
            export_path(None, Some("2026-11-30")).unwrap().0,
            format!("{EXPORT_PATH}?to=2026-11-30")
        );
        for value in [
            "2026-1-01",
            "2026-02-30",
            "2026-11-20T00:00:00Z",
            "2026-11-01&status=paid",
        ] {
            assert!(export_path(Some(value), None).is_err());
        }
        assert!(export_path(Some("2026-12-01"), Some("2026-11-30")).is_err());
    }

    #[test]
    fn csv_bytes_preserve_quoting_multiline_japanese_and_formula_protection() {
        let text = "\u{feff}reservation_id,reservation_number,reservation_type_id,customer_name\r\nr1,R1,t1,\"山田,太郎\n\t=SUM(1,2)\"\r\n";
        assert_eq!(
            csv_text(text.as_bytes().to_vec(), "text/csv; charset=utf-8").unwrap(),
            text
        );
    }

    #[test]
    fn html_errors_invalid_utf8_and_oversized_reports_are_not_saved_as_csv() {
        assert!(csv_text(b"<html>login</html>".to_vec(), "text/html").is_err());
        assert!(csv_text(br#"{"error":"denied"}"#.to_vec(), "text/csv").is_err());
        assert!(csv_text(vec![0xff, 0xfe], "text/csv").is_err());
        assert!(csv_text(vec![b'x'; MAX_CSV_BYTES + 1], "text/csv").is_err());
    }

    #[test]
    fn export_filename_cannot_escape_its_temporary_directory() {
        let expected = "reservations_2026-11-20.csv";
        assert_eq!(
            response_filename(
                Some("attachment; filename=\"reservations_2026-11-20.csv\""),
                "fallback.csv"
            ),
            expected
        );
        for unsafe_name in [
            "../../reservations_x.csv",
            "reservations_../x.csv",
            "reservations_\\x.csv",
            "reservations_x.csv\n",
            "reservations_x.html",
        ] {
            assert_eq!(
                response_filename(
                    Some(&format!("attachment; filename=\"{unsafe_name}\"")),
                    "fallback.csv"
                ),
                "fallback.csv"
            );
        }
        assert_eq!(response_filename(None, "fallback.csv"), "fallback.csv");
    }
}
