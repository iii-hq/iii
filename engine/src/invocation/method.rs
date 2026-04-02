// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HttpAuth {
    Hmac { secret: String },
    Bearer { token: String },
    ApiKey { header: String, value: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_method_serialize_roundtrip() {
        for (variant, expected) in [
            (HttpMethod::Get, "\"GET\""),
            (HttpMethod::Post, "\"POST\""),
            (HttpMethod::Put, "\"PUT\""),
            (HttpMethod::Patch, "\"PATCH\""),
            (HttpMethod::Delete, "\"DELETE\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected);
            let back: HttpMethod = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&back).unwrap(), expected);
        }
    }

    #[test]
    fn http_auth_hmac_roundtrip() {
        let auth = HttpAuth::Hmac { secret: "s".into() };
        let json = serde_json::to_value(&auth).unwrap();
        assert_eq!(json["type"], "hmac");
        assert_eq!(json["secret"], "s");
        let back: HttpAuth = serde_json::from_value(json).unwrap();
        assert!(matches!(back, HttpAuth::Hmac { secret } if secret == "s"));
    }

    #[test]
    fn http_auth_bearer_roundtrip() {
        let auth = HttpAuth::Bearer { token: "t".into() };
        let json = serde_json::to_value(&auth).unwrap();
        assert_eq!(json["type"], "bearer");
        let back: HttpAuth = serde_json::from_value(json).unwrap();
        assert!(matches!(back, HttpAuth::Bearer { token } if token == "t"));
    }

    #[test]
    fn http_auth_api_key_roundtrip() {
        let auth = HttpAuth::ApiKey {
            header: "X-Key".into(),
            value: "v".into(),
        };
        let json = serde_json::to_value(&auth).unwrap();
        assert_eq!(json["type"], "apikey");
        let back: HttpAuth = serde_json::from_value(json).unwrap();
        assert!(
            matches!(back, HttpAuth::ApiKey { header, value } if header == "X-Key" && value == "v")
        );
    }
}
