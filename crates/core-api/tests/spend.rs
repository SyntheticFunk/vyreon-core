//! C-078: core reads DigitalOcean's billing with the owner's read-only token from a file, at each read.
//! A simulated DigitalOcean on this machine answers with the API's documented shapes and the account's
//! real figures (month to date $269.04, 2026-11-24 06:42 UTC); no token, an empty file and a refusal are
//! each said plainly, and the token is never in what is said.
use core_api::spend::Billing;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

const TOKEN: &str = "dop_v1_simulatedtokenfortests";

/// What the simulated DigitalOcean saw: each request's path and whether it carried the token.
type Seen = Arc<Mutex<Vec<(String, bool)>>>;

/// A DigitalOcean answering each request with (status, body) in turn; records each request's path and
/// whether it carried the token as its bearer.
fn digitalocean(answers: Vec<(u16, &'static str)>) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    std::thread::spawn(move || {
        for (mut stream, (status, body)) in listener.incoming().flatten().zip(answers) {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let path = line.split_whitespace().nth(1).unwrap_or_default().to_owned();
            let mut bearer = false;
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                if h.trim().is_empty() {
                    break;
                }
                bearer |= h.trim().eq_ignore_ascii_case(&format!("authorization: Bearer {TOKEN}"));
            }
            kept.lock().unwrap().push((path, bearer));
            let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    (url, seen)
}

#[tokio::test]
async fn the_billing_is_read_with_the_token_from_its_file_and_its_absence_is_said() {
    let dir = std::env::temp_dir().join(format!("vyreon-spend-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("token");

    // no setting, then a file not made (the secret absent): said, no request sent
    let (url, seen) = digitalocean(vec![]);
    let e = Billing::new(None, &url).unwrap().read("2026-11").await.unwrap_err();
    assert!(e.starts_with("no DigitalOcean token: VYREON_CORE_DO_TOKEN_FILE is not set"), "{e}");
    let e = Billing::new(Some(file.clone()), &url).unwrap().read("2026-11").await.unwrap_err();
    assert!(e.starts_with("no DigitalOcean token: the secret vyreon-core-do-billing is not made"), "{e}");
    assert!(seen.lock().unwrap().is_empty());

    // the token refused: the status said, the token never
    std::fs::write(&file, format!("{TOKEN}\n")).unwrap();
    let (url, _) = digitalocean(vec![(401, r#"{"id": "Unauthorized", "message": "Unable to authenticate you"}"#)]);
    let e = Billing::new(Some(file.clone()), &url).unwrap().read("2026-11").await.unwrap_err();
    assert_eq!(e, "GET /v2/customers/my/balance: DigitalOcean answered 401 Unauthorized (the token is refused: is it valid, with billing:read?)");
    assert!(!e.contains(TOKEN));

    // read: the balance's month-to-date figure and the invoices' finished months, the token as bearer
    let (url, seen) = digitalocean(vec![
        (200, r#"{"month_to_date_balance": "269.04", "account_balance": "0.00", "month_to_date_usage": "269.04", "generated_at": "2026-11-24T06:42:00Z"}"#),
        (200, r#"{"invoices": [{"invoice_uuid": "u", "amount": "212.50", "invoice_period": "2026-10", "updated_at": "2026-11-01T08:00:00Z"}], "invoice_preview": {"amount": "269.04", "invoice_period": "2026-11"}, "links": {}, "meta": {"total": 1}}"#),
    ]);
    let r = Billing::new(Some(file.clone()), &url).unwrap().read("2026-11").await.unwrap();
    assert_eq!((r.month.as_str(), r.month_to_date_usd, r.invoices.clone()), ("2026-11", 269.04, vec![("2026-10".to_owned(), 212.5)]));
    assert_eq!(
        seen.lock().unwrap().clone(),
        [("/v2/customers/my/balance".to_owned(), true), ("/v2/customers/my/invoices?per_page=200".to_owned(), true)]
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
