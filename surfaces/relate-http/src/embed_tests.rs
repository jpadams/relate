//! Embedding host: a non-TCP stream, a custom authenticator, and an owner
//! that hides another caller's transaction (ADR-0428).

use std::io::{self, Cursor, Read, Write};
use std::sync::{Arc, Mutex};

use relate::Database;
use relate_auth::{AuthConfig, AuthError, Authenticator};

use crate::http::Request;
use crate::reply::Reply;
use crate::server::{serve_connection, HttpConfig, HttpExtension};

struct Mem {
    input: Cursor<Vec<u8>>,
    output: Arc<Mutex<Vec<u8>>>,
}

impl Read for Mem {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.input.read(buf)
    }
}

impl Write for Mem {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.output.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct TokenAuth;

impl Authenticator for TokenAuth {
    fn verify_credential(&self, credential: &str) -> Result<(), AuthError> {
        if matches!(credential, "tok" | "ada" | "grace") {
            Ok(())
        } else {
            Err(AuthError::Rejected)
        }
    }

    fn transaction_owner(
        &self,
        credential: &str,
        _header_value: Option<&str>,
    ) -> Result<Option<String>, AuthError> {
        match credential {
            "ada" | "grace" => Ok(Some(credential.to_string())),
            "tok" => Ok(None),
            _ => Err(AuthError::Rejected),
        }
    }
}

struct OpenHealth;

impl HttpExtension for OpenHealth {
    fn is_public(&self, _method: &str, _path: &str) -> bool {
        true
    }

    fn handle(&self, request: &Request) -> Option<Reply> {
        if request.path == "/health" {
            Some(Reply::from_json(200, serde_json::json!({"status": "ok"})))
        } else {
            None
        }
    }
}

fn config() -> HttpConfig {
    HttpConfig::new(AuthConfig::Custom(Arc::new(TokenAuth)))
}

fn request(method: &str, path: &str, credential: &str, body: &str) -> Vec<u8> {
    let headers = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {credential}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut bytes = headers.into_bytes();
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

fn roundtrip(config: &HttpConfig, db: &Database, raw: Vec<u8>) -> String {
    let output = Arc::new(Mutex::new(Vec::new()));
    let stream = Mem {
        input: Cursor::new(raw),
        output: Arc::clone(&output),
    };
    serve_connection(db, stream, config).unwrap();
    let bytes = output.lock().unwrap().clone();
    String::from_utf8(bytes).unwrap()
}

fn status(response: &str) -> u16 {
    response.split_whitespace().nth(1).unwrap().parse().unwrap()
}

fn transaction_id(response: &str) -> String {
    let body = response.split("\r\n\r\n").nth(1).expect("response body");
    serde_json::from_str::<serde_json::Value>(body).expect("JSON response")["transaction"]["id"]
        .as_str()
        .expect("transaction id")
        .to_string()
}

#[test]
fn custom_auth_runs_a_query_on_a_memory_stream() {
    let db = Database::open(":memory:").unwrap();
    let config = config();
    let ok = roundtrip(
        &config,
        &db,
        request(
            "POST",
            "/db/neo4j/query/v2",
            "tok",
            r#"{"statement":"RETURN 1 AS n"}"#,
        ),
    );
    assert_eq!(status(&ok), 200, "{ok}");
    assert!(ok.contains("\"n\""));

    let denied_body = r#"{"statement":"RETURN 1"}"#;
    let denied = roundtrip(
        &config,
        &db,
        format!(
            "POST /db/neo4j/query/v2 HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAuthorization: Bearer nope\r\nContent-Length: {}\r\n\r\n{denied_body}",
            denied_body.len()
        )
        .into_bytes(),
    );
    assert_eq!(status(&denied), 401, "{denied}");
}

#[test]
fn a_different_owner_cannot_see_the_transaction() {
    let db = Database::open(":memory:").unwrap();
    let config = config();
    let begun = roundtrip(
        &config,
        &db,
        request("POST", "/db/neo4j/query/v2/tx", "ada", "{}"),
    );
    assert_eq!(status(&begun), 201, "{begun}");
    let id = transaction_id(&begun);
    let hidden = roundtrip(
        &config,
        &db,
        request(
            "POST",
            &format!("/db/neo4j/query/v2/tx/{id}"),
            "grace",
            r#"{"statement":"RETURN 1"}"#,
        ),
    );
    assert_eq!(status(&hidden), 404, "{hidden}");
    let spoof_body = r#"{"statement":"RETURN 1"}"#;
    let spoofed = roundtrip(
        &config,
        &db,
        format!(
            "POST /db/neo4j/query/v2/tx/{id} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer grace\r\nx-owner: ada\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{spoof_body}",
            spoof_body.len()
        )
        .into_bytes(),
    );
    assert_eq!(status(&spoofed), 404, "{spoofed}");
    let rolled = roundtrip(
        &config,
        &db,
        request("DELETE", &format!("/db/neo4j/query/v2/tx/{id}"), "ada", ""),
    );
    assert_eq!(status(&rolled), 202, "{rolled}");
}

#[test]
fn a_public_extension_does_not_unauthenticate_a_query() {
    let db = Database::open(":memory:").unwrap();
    let mut config = config();
    config.extension = Some(Arc::new(OpenHealth));
    let health = roundtrip(
        &config,
        &db,
        b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n".to_vec(),
    );
    assert_eq!(status(&health), 200, "{health}");
    let body = r#"{"statement":"RETURN 1"}"#;
    let query = roundtrip(
        &config,
        &db,
        format!(
            "POST /db/neo4j/query/v2 HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes(),
    );
    assert_eq!(status(&query), 401, "{query}");
}
