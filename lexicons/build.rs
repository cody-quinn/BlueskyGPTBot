#![allow(warnings)]

#[path = "build/spec.rs"]
mod spec;

#[path = "build/casing.rs"]
mod casing;

use std::path::Path;
use std::{env, fs};

use casing::convert_casing_to_pascal;
use spec::{
    Lexicon, LexiconDoc, LexiconObject, LexiconPrimitive, LexiconType, LexiconXrpcQueryProc,
};

impl Lexicon {
    fn codegen(&self) -> String {
        match &self.typ {
            LexiconType::Token => todo!(),
            LexiconType::Object { inner } => {
                let name = self
                    .id
                    .split('.')
                    .last()
                    .map(convert_casing_to_pascal)
                    .unwrap();

                let mut result = String::new();
                result.push_str(&format!("pub struct {name} {{\n"));
                result.push_str(&self.codegen_object(inner));
                result.push_str("}\n");
                result
            }
            LexiconType::Record { inner: _ } => todo!(),
            LexiconType::Query { inner } => self.codegen_queryproc(inner),
            LexiconType::Procedure { inner } => self.codegen_queryproc(inner),
            LexiconType::Blob => todo!(),
            LexiconType::Image => todo!(),
            LexiconType::Video => todo!(),
            LexiconType::Audio => todo!(),
        }
    }

    fn codegen_queryproc(&self, procedure: &LexiconXrpcQueryProc) -> String {
        let mut result = String::new();

        // Get the name of the structure
        let name = self
            .id
            .split('.')
            .last()
            .map(convert_casing_to_pascal)
            .unwrap();

        if let Some(body) = &procedure.parameters {
            result.push_str(&format!("pub struct {name}Params {{\n"));
            let object = self.codegen_object(&body.properties.clone().into());
            result.push_str(&object);
            result.push_str("}\n");
        }

        if let Some(body) = &procedure.input {
            result.push_str(&format!("pub struct {name}Input {{\n"));
            let object = self.codegen_object(&body.schema);
            result.push_str(&object);
            result.push_str("}\n");
        }

        if let Some(body) = &procedure.output {
            result.push_str(&format!("pub struct {name}Output {{\n"));
            let object = self.codegen_object(&body.schema);
            result.push_str(&object);
            result.push_str("}\n");
        }

        result
    }

    fn codegen_object(&self, object: &LexiconObject) -> String {
        let mut result = String::new();

        for (name, prop) in object.properties.iter() {
            result.push_str("    pub ");
            result.push_str(name);
            result.push_str(": ");

            let typ = match prop {
                LexiconPrimitive::Boolean => "bool",
                LexiconPrimitive::Number => "f64",
                LexiconPrimitive::Integer => "i64",
                LexiconPrimitive::String { enum_values: _ } => "String",
            };

            if object.required.contains(name) {
                result.push_str(typ);
            } else {
                result.push_str("Option<");
                result.push_str(typ);
                result.push('>');
            }

            result.push_str(",\n");
        }

        result
    }
}

fn main() {
    let root = env::var("CARGO_MANIFEST_DIR").unwrap();

    let in_path = Path::new(&root).join("data/");
    let out_path = Path::new(&root).join("src/");

    let lexicon_file =
        fs::read_to_string(in_path.join("com/atproto/server/createSession.json")).unwrap();
    let lexicon_file = serde_json::from_str::<LexiconDoc>(&lexicon_file).unwrap();
    // println!();

    // panic!("{}", lexicon_file.lexicons().first().unwrap().codegen());
}
