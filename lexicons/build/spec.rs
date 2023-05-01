use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LexiconDoc {
    lexicon: i32,
    id: String,
    description: Option<String>,

    defs: HashMap<String, Value>,
}

impl LexiconDoc {
    pub fn lexicons(&self) -> Vec<Lexicon> {
        let mut lexicons = Vec::new();

        for (name, def) in self.defs.iter() {
            let mut id = self.id.clone();
            if name != "main" {
                id.push_str(name);
            }

            // Parsing the lexicon and updating the id
            // panic!("{:#?}", def.clone());
            let mut lexicon = serde_json::from_value::<Lexicon>(def.clone()).unwrap();
            lexicon.id = id;

            lexicons.push(lexicon);
        }

        lexicons
    }
}

// Core
// =

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lexicon {
    #[serde(default)]
    pub id: String,

    pub revision: Option<i32>,
    pub description: Option<String>,

    #[serde(flatten)]
    pub typ: LexiconType,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LexiconType {
    Token,
    Object {
        #[serde(flatten)]
        inner: LexiconObject,
    },
    Record {
        #[serde(flatten)]
        inner: LexiconRecord,
    },
    Query {
        #[serde(flatten)]
        inner: LexiconXrpcQueryProc,
    },
    Procedure {
        #[serde(flatten)]
        inner: LexiconXrpcQueryProc,
    },
    Blob,
    Image,
    Video,
    Audio,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LexiconObject {
    #[serde(default)]
    pub required: Vec<String>,
    pub properties: HashMap<String, LexiconPrimitive>,
}

impl From<HashMap<String, LexiconPrimitive>> for LexiconObject {
    fn from(value: HashMap<String, LexiconPrimitive>) -> Self {
        Self {
            required: Vec::default(),
            properties: value,
        }
    }
}

// Database
// =

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LexiconRecord {
    key: Option<String>,
    record: LexiconObject,
}

// XRPC
// =

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LexiconXrpcQueryProc {
    #[serde(default)]
    pub parameters: Option<XrpcParameters>,
    pub input: Option<XrpcBody>,
    pub output: Option<XrpcBody>,
    #[serde(default)]
    pub errors: Vec<XrpcError>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct XrpcParameters {
    #[serde(rename = "type")]
    pub typ: String,
    pub properties: HashMap<String, LexiconPrimitive>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct XrpcBody {
    pub encoding: String,
    pub schema: LexiconObject,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct XrpcError {
    pub name: String,
    pub description: Option<String>,
}

// Primitives
// =

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LexiconPrimitive {
    Boolean,
    Number,
    Integer,
    String {
        #[serde(rename = "enum")]
        enum_values: Option<Vec<String>>,
    },
}

// FIXME
// type XrpcParameter = Value;
// type XrpcBody = Value;
// type XrpcError = Value;
