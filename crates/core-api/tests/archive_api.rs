//! The perishable archive (C-052) over HTTP: a day block asks core for an upload URL, PUTs the
//! archive to a simulated S3 endpoint on this machine (no network) that checks the request as S3
//! does (the Signature Version 4 query signature recomputed with the bucket's key, the signed
//! headers sent as signed, the URL's expiry, Content-MD5 against the body), reports it, and core
//! reads the object back before it is kept. The archive is the engine's packing of the real ACT01
//! interval pulls of 2025-12-19 (`vyreon-worker pack-perishable`, engine pass-fx26); the day is the
//! real slice of 2025-12-10 (core treats the archive as opaque bytes). Run with
//! `cargo test -- --ignored`.
use core_api::{App, Config};
use core_contract::sha256_hex;
use core_db::archive::{self, Spaces};
use core_db::budget::Budget;
use core_db::work::{self, Kind};
use core_db::{build, migrate, universe};
use md5::{Digest, Md5};
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use test_support::slice::{BASE, bundle, inventories, parts};
use test_support::{Scratch, TAPE_AAPL, fixtures};

const PROFILE: &str = "FREE_REPORT_64";
const WORKERS_TOKEN: &str = "the workers' shared token of this test";
const KEY_ID: &str = "DO00ARCHIVEAPITESTKEY";
const SECRET: &str = "the-bucket-secret-of-this-test";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Unix seconds of `YYYYMMDDTHHMMSSZ` (the proleptic Gregorian calendar, days from civil).
fn unix(amz: &str) -> i64 {
    let n = |r: std::ops::Range<usize>| amz[r].parse::<i64>().unwrap();
    let (y, m, d) = (n(0..4), n(4..6), n(6..8));
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    days * 86_400 + n(9..11) * 3600 + n(11..13) * 60 + n(13..15)
}

struct Object {
    body: Vec<u8>,
    sha256: String,
}

/// A simulated S3 endpoint holding bucket `vyreon-data`, checking each request as S3 does.
/// `skew` moves its clock (seconds), to see an expired URL refused.
struct Bucket {
    url: String,
    objects: Arc<Mutex<BTreeMap<String, Object>>>,
    skew: Arc<AtomicI64>,
    /// Every request target it received (a URL with its signature: what must not leak elsewhere).
    seen: Arc<Mutex<Vec<String>>>,
}

/// A request as received: method, target, headers (lowercase names), body.
type Received = (String, String, BTreeMap<String, String>, Vec<u8>);

fn read_request(stream: &TcpStream) -> Option<Received> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next()?.to_owned(), parts.next()?.to_owned());
    let mut headers = BTreeMap::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':')?;
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
    }
    let mut body = vec![0u8; if method == "HEAD" { 0 } else { headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0) }];
    reader.read_exact(&mut body).ok()?;
    Some((method, target, headers, body))
}

fn bucket() -> Bucket {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let host = url.trim_start_matches("http://").to_owned();
    let (objects, skew, seen) = (Arc::new(Mutex::new(BTreeMap::<String, Object>::new())), Arc::new(AtomicI64::new(0)), Arc::new(Mutex::new(Vec::new())));
    let (o, k, s) = (objects.clone(), skew.clone(), seen.clone());
    let keyholder = Spaces::new(&url, "nyc3", "vyreon-data", KEY_ID, SECRET).unwrap();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let Some((method, target, headers, body)) = read_request(&stream) else { continue };
            s.lock().unwrap().push(target.clone());
            let (path, query) = target.split_once('?').unwrap_or((&target, ""));
            let params: BTreeMap<&str, &str> = query.split('&').filter_map(|p| p.split_once('=')).collect();
            let decode = |v: &str| v.replace("%2F", "/").replace("%3B", ";");
            let signed: Vec<String> = params.get("X-Amz-SignedHeaders").map(|v| decode(v).split(';').map(str::to_owned).collect()).unwrap_or_default();
            // a HEAD's Content-Length is the object's; its body is never sent
            let mut reply = |status: u16, extra: &str, body: &str, head_length: usize| {
                let (length, body) = if method == "HEAD" { (head_length, "") } else { (body.len(), body) };
                let _ = write!(stream, "HTTP/1.1 {status} X\r\n{extra}Content-Length: {length}\r\nConnection: close\r\n\r\n{body}");
            };
            // the signature, recomputed from what was received
            let date = params.get("X-Amz-Date").copied().unwrap_or_default();
            let expires: i32 = params.get("X-Amz-Expires").and_then(|e| e.parse().ok()).unwrap_or(0);
            let given: Vec<(String, String)> = signed.iter().map(|h| (h.clone(), if h == "host" { host.clone() } else { headers.get(h).cloned().unwrap_or_default() })).collect();
            // the same request signed again with the bucket's key (the signer core uses)
            let key = path.trim_start_matches("/vyreon-data/").to_owned();
            let unsigned: Vec<(&str, &str)> = given.iter().filter(|(h, _)| h != "host").map(|(h, v)| (h.as_str(), v.as_str())).collect();
            let resigned = match (method.as_str(), date.len() == 16) {
                ("PUT", true) => keyholder.presign_put(&key, &unsigned, unix(date), expires),
                ("HEAD", true) if unsigned.is_empty() => keyholder.presign_head(&key, unix(date), expires),
                _ => String::new(),
            };
            let expected = resigned.split_once('?').map_or("", |(_, q)| q).to_owned();
            if date.len() != 16 || expected != query {
                reply(403, "", "<Error><Code>SignatureDoesNotMatch</Code></Error>", 0);
                continue;
            }
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64 + k.load(Ordering::Relaxed);
            if now > unix(date) + i64::from(expires) {
                reply(403, "", "<Error><Code>AccessDenied</Code><Message>Request has expired</Message></Error>", 0);
                continue;
            }
            match method.as_str() {
                "PUT" => {
                    let md5 = Md5::digest(&body);
                    let b64 = {
                        use std::fmt::Write as _;
                        // the store's own Content-MD5 check: base64 of the body's MD5
                        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
                        let mut out = String::new();
                        for c in md5.chunks(3) {
                            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
                            for i in 0..4 {
                                let _ = out.write_char(if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
                            }
                        }
                        out
                    };
                    if headers.get("content-md5") != Some(&b64) {
                        reply(400, "", "<Error><Code>BadDigest</Code></Error>", 0);
                        continue;
                    }
                    o.lock().unwrap().insert(key, Object { body, sha256: headers.get("x-amz-meta-sha256").cloned().unwrap_or_default() });
                    reply(200, &format!("ETag: \"{}\"\r\n", hex(&md5)), "", 0);
                }
                "HEAD" => {
                    let held = o.lock().unwrap().get(&key).map(|obj| (format!("ETag: \"{}\"\r\nx-amz-meta-sha256: {}\r\n", hex(&Md5::digest(&obj.body)), obj.sha256), obj.body.len()));
                    match held {
                        Some((extra, length)) => reply(200, &extra, "", length),
                        None => reply(404, "", "", 0),
                    }
                }
                _ => reply(405, "", "", 0),
            }
        }
    });
    Bucket { url, objects, skew, seen }
}

struct Service {
    scratch: Scratch,
    /// The service with the archive on, and the same service with it off.
    url: String,
    off: String,
    http: reqwest::Client,
    base_build: String,
}

async fn service(test: &str, bucket: &Bucket) -> Service {
    let scratch = Scratch::new(test).await;
    let mut client = scratch.client().await;
    migrate::apply(&mut client, &migrate::embedded().unwrap()).await.unwrap();
    work::load_calendar(&mut client, &serde_json::from_str(test_support::CALENDAR).unwrap()).await.unwrap();
    let doc = universe::read_responses(&fixtures().join("engine-v1-PROFILE.json"), &fixtures().join("intrinio-universe")).unwrap();
    universe::store(&mut client, &doc).await.unwrap();
    let mut builds = Vec::new();
    for inventory in inventories() {
        let b = build::register(&mut client, &inventory).await.unwrap().engine_build;
        build::admit(&client, &b, &json!({"checklist": "archive API test"})).await.unwrap();
        builds.push(b);
    }
    let mut urls = Vec::new();
    for archive in [Some(Spaces::new(&bucket.url, "nyc3", "vyreon-data", KEY_ID, SECRET).unwrap()), None] {
        let pool = deadpool_postgres::Pool::builder(deadpool_postgres::Manager::new(scratch.config.clone(), tokio_postgres::NoTls)).max_size(4).build().unwrap();
        let config = Config { worker_token_sha256: sha256_hex(WORKERS_TOKEN.as_bytes()), spool: std::env::temp_dir(), lease_seconds: 300.0, budget: Budget::default(), max_bundle_bytes: 1 << 20, archive };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        urls.push(format!("http://{}", listener.local_addr().unwrap()));
        let app = App::new(pool, config);
        tokio::spawn(async move { axum::serve(listener, core_api::router(app)).await.unwrap() });
    }
    Service { scratch, off: urls.pop().unwrap(), url: urls.pop().unwrap(), http: reqwest::Client::new(), base_build: builds.remove(0) }
}

impl Service {
    async fn post(&self, base: &str, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
        let r = self.http.post(format!("{base}/worker/v1{path}")).bearer_auth(token).json(&body).send().await.unwrap();
        (r.status(), r.json().await.unwrap_or(Value::Null))
    }
}

fn asked(category: &str, bytes: &[u8]) -> Value {
    json!({"category": category, "session": "2025-12-10", "bytes": bytes.len(), "sha256": sha256_hex(bytes), "md5": hex(&Md5::digest(bytes))})
}

/// PUTs `body` to the URL core gave, with the headers core named.
async fn upload(http: &reqwest::Client, grant: &Value, body: &[u8]) -> StatusCode {
    let mut put = http.put(grant["url"].as_str().unwrap()).body(body.to_vec());
    for (k, v) in grant["headers"].as_object().unwrap() {
        put = put.header(k.as_str(), v.as_str().unwrap());
    }
    put.send().await.unwrap().status()
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_worker_archives_a_published_days_perishables_through_a_url_core_signs() {
    let b = bucket();
    let s = service("archive_api", &b).await;
    let archive = std::fs::read(fixtures().join("perishable-act01-auto-2025-12-19.tar.zst")).unwrap();
    assert_eq!(sha256_hex(&archive), "4ef2e74513ada5566703f33149542eee0c18423f181079e4b3625766054429fd");
    let mut admin = s.scratch.client().await;
    work::schedule(&mut admin, Kind::DayBlock, PROFILE, &s.base_build, "2025-12-10", "2025-12-10", 30).await.unwrap();
    let claim: Value = s
        .http
        .post(format!("{}/worker/v1/claims", s.url))
        .bearer_auth(WORKERS_TOKEN)
        .json(&json!({"kind": "day-block", "engine_build": s.base_build, "worker": "day-1"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let token = claim["lease_token"].as_str().unwrap();

    // off: every request is answered so, still only under a live lease
    assert_eq!(s.post(&s.off, "/archives", token, asked("act01-auto", &archive)).await, (StatusCode::OK, json!({"state": "off"})));
    assert_eq!(s.post(&s.off, "/archives", "not-a-lease", asked("act01-auto", &archive)).await.0, StatusCode::GONE);
    // a day not published archives nothing
    let (status, early) = s.post(&s.url, "/archives", token, asked("act01-auto", &archive)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{early}");
    let (header, scalars, files) = parts(BASE);
    let path = bundle("archive-api-base", &header, &scalars, &files);
    let published = s.http.post(format!("{}/worker/v1/days", s.url)).bearer_auth(token).body(std::fs::read(&path).unwrap()).send().await.unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(published.status(), StatusCode::OK);
    // published, not yet said done (round x, 9): not yet a day the job has published
    assert_eq!(s.post(&s.url, "/archives", token, asked("act01-auto", &archive)).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(s.post(&s.url, "/days/2025-12-10/done", token, json!({})).await.0, StatusCode::OK);

    // the URL core signs is taken by the bucket; core reads the object back and keeps it
    let (status, grant) = s.post(&s.url, "/archives", token, asked("act01-auto", &archive)).await;
    assert_eq!((status, grant["state"].as_str()), (StatusCode::OK, Some("upload")), "{grant}");
    let key = format!("perishables/act01-auto/2025-12-10/{}.tar.zst", sha256_hex(&archive));
    assert_eq!(grant["key"], key);
    assert!(grant["url"].as_str().unwrap().starts_with(&format!("{}/vyreon-data/{key}?", b.url)));
    // what is sent must be what was signed: another SHA-256 header, another body
    let mut forged = grant.clone();
    forged["headers"][archive::META_SHA256] = json!(sha256_hex(TAPE_AAPL));
    assert_eq!(upload(&s.http, &forged, &archive).await, StatusCode::FORBIDDEN);
    assert_eq!(upload(&s.http, &grant, &TAPE_AAPL[..archive.len()]).await, StatusCode::BAD_REQUEST);
    assert_eq!(upload(&s.http, &grant, &archive).await, StatusCode::OK);
    let id = grant["archive_id"].as_i64().unwrap();
    let (status, done) = s.post(&s.url, &format!("/archives/{id}/done"), token, json!({"failed": null})).await;
    assert_eq!((status, done["kept"].as_bool()), (StatusCode::OK, Some(true)), "{done}");
    assert_eq!(b.objects.lock().unwrap()[&key].body, archive);
    // again: kept, no URL
    let (_, again) = s.post(&s.url, "/archives", token, asked("act01-auto", &archive)).await;
    assert_eq!(again, json!({"state": "kept", "archive_id": id, "key": key}));

    // a URL used after it expired is refused by the bucket; the worker says so; not kept
    let prints = TAPE_AAPL;
    let (_, late) = s.post(&s.url, "/archives", token, asked("exq01-events", prints)).await;
    b.skew.store(i64::from(archive::URL_SECONDS) + 60, Ordering::Relaxed);
    assert_eq!(upload(&s.http, &late, prints).await, StatusCode::FORBIDDEN);
    b.skew.store(0, Ordering::Relaxed);
    let late_id = late["archive_id"].as_i64().unwrap();
    let (_, said) = s.post(&s.url, &format!("/archives/{late_id}/done"), token, json!({"failed": "the bucket answered 403 (AccessDenied)"})).await;
    assert_eq!((said["kept"].as_bool(), said["why"].as_str()), (Some(false), Some("the worker: the bucket answered 403 (AccessDenied)")));
    // reported done without an upload: the bucket does not hold it
    let (_, retry) = s.post(&s.url, "/archives", token, asked("exq01-events", prints)).await;
    assert_eq!(retry["archive_id"].as_i64(), Some(late_id));
    let (_, nothing) = s.post(&s.url, &format!("/archives/{late_id}/done"), token, json!({})).await;
    assert_eq!((nothing["kept"].as_bool(), nothing["why"].as_str()), (Some(false), Some("the object is not in the bucket")));
    // another job's archive is not this lease's to report
    let (status, _) = s.post(&s.url, "/archives/999999/done", token, json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // round w, 7: a category the worker could not pack is reported (off: answered off)
    let unpacked = json!({"category": "exq01-events", "session": "2025-12-10", "why": "not packed: zstd killed at the deadline"});
    assert_eq!(s.post(&s.off, "/archives/unpacked", token, unpacked.clone()).await, (StatusCode::OK, json!({"state": "off"})));
    assert_eq!(s.post(&s.url, "/archives/unpacked", token, unpacked).await, (StatusCode::OK, json!({"recorded": true})));
    let reported: i64 = admin.query_one("SELECT count(*) FROM core.perishable_unpacked", &[]).await.unwrap().get(0);
    assert_eq!(reported, 1);

    // no URL the bucket received is in core's database
    let rows: String = admin.query_one("SELECT COALESCE(string_agg(row_to_json(a)::text, ' '), '') FROM core.perishable_archive a", &[]).await.unwrap().get(0);
    for target in b.seen.lock().unwrap().iter() {
        let signature = target.rsplit_once("X-Amz-Signature=").map_or(target.as_str(), |(_, sig)| sig);
        assert!(!rows.contains(signature), "a signature is stored");
    }
    assert!(!rows.contains("X-Amz") && !rows.contains(SECRET) && !rows.contains(KEY_ID));
    let states: Vec<(String, String)> =
        admin.query("SELECT category, state FROM core.perishable_archive ORDER BY archive_id", &[]).await.unwrap().iter().map(|r| (r.get(0), r.get(1))).collect();
    assert_eq!(states, [("act01-auto".to_owned(), "KEPT".to_owned()), ("exq01-events".to_owned(), "FAILED".to_owned())]);
    drop(admin);
    s.scratch.drop().await;
}
