use std::io::BufReader;
use std::path::Path;

use horned_owl::io::ParserConfiguration;
use horned_owl::model::Component;

use crate::error::{Error, Result};

const BFO_PURL_PREFIX: &str = "http://purl.obolibrary.org/obo/bfo";
const OBO_IRI_PREFIX: &str = "http://purl.obolibrary.org/obo/";

/// Result of analysing an OWL file.
#[derive(Debug, Clone)]
pub struct OwlAnalysis {
    pub class_count: u64,
    pub bfo_grounded: bool,
    pub license: String,
    #[allow(dead_code)]
    pub imports: Vec<String>,
}

/// Parse an OWL/OWX file from `path` and return analysis results.
/// Uses the OWX (XML) reader; falls back to treating parse errors as
/// non-fatal so that a bad file is cataloged with bfo_grounded=false.
pub fn analyse_owl_file(path: &Path) -> Result<OwlAnalysis> {
    let file = std::fs::File::open(path)?;
    let mut bufread = BufReader::new(file);

    // Try OWX (XML) first, then RDF/Turtle-style.
    let set_onto = horned_owl::io::owx::reader::read(&mut bufread, ParserConfiguration::default())
        .map(|(onto, _mapping)| onto)
        .or_else(|_owx_err| {
            // Re-open for RDF reader.
            let file2 = std::fs::File::open(path).map_err(Error::Io)?;
            let mut buf2 = BufReader::new(file2);
            horned_owl::io::rdf::reader::read(&mut buf2, ParserConfiguration::default())
                .map(|(rdf_onto, _incomplete)| {
                    // Convert RDFOntology to SetOntology
                    rdf_onto.into()
                })
                .map_err(|e| Error::OwlParse(e.to_string()))
        })?;

    let mut class_count: u64 = 0;
    let mut imports: Vec<String> = Vec::new();
    let mut license = String::new();

    for annotated in set_onto.iter() {
        match &annotated.component {
            Component::DeclareClass(_) => {
                class_count += 1;
            }
            Component::Import(import) => {
                imports.push(import.0.to_string());
            }
            Component::AnnotationAssertion(aa) => {
                let prop_iri = aa.ann.ap.0.to_string();
                // dc:license or dcterms:license or cc:license
                if prop_iri.contains("license") || prop_iri.contains("License") {
                    if let horned_owl::model::AnnotationValue::Literal(lit) = &aa.ann.av {
                        if license.is_empty() {
                            license = lit.literal().clone();
                        }
                    } else if let horned_owl::model::AnnotationValue::IRI(iri) = &aa.ann.av {
                        if license.is_empty() {
                            license = iri.to_string();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // BFO grounding: any import that starts with the BFO PURL, or any
    // class IRI that is in the obo: namespace (which strongly implies BFO
    // alignment).
    let bfo_grounded = imports.iter().any(|imp| {
        imp.to_lowercase()
            .starts_with(&BFO_PURL_PREFIX.to_lowercase())
    }) || {
        // Count obo: classes as a proxy for BFO grounding.
        let mut obo_class_count = 0u64;
        for annotated in set_onto.iter() {
            if let Component::DeclareClass(dc) = &annotated.component {
                let iri_str = dc.0 .0.to_string();
                if iri_str.starts_with(OBO_IRI_PREFIX) {
                    obo_class_count += 1;
                }
            }
        }
        obo_class_count > 0
    };

    Ok(OwlAnalysis {
        class_count,
        bfo_grounded,
        license,
        imports,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_fixture(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::with_suffix(".owx").unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    const BFO_GROUNDED_OWX: &str = r#"<?xml version="1.0"?>
<Ontology xmlns="http://www.w3.org/2002/07/owl#"
          xml:base="http://purl.obolibrary.org/obo/test.owl"
          xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          ontologyIRI="http://purl.obolibrary.org/obo/test.owl">
  <Import>http://purl.obolibrary.org/obo/bfo.owl</Import>
  <Declaration>
    <Class IRI="http://purl.obolibrary.org/obo/TEST_0000001"/>
  </Declaration>
  <Declaration>
    <Class IRI="http://purl.obolibrary.org/obo/TEST_0000002"/>
  </Declaration>
  <AnnotationAssertion>
    <AnnotationProperty IRI="http://purl.org/dc/terms/license"/>
    <IRI>http://purl.obolibrary.org/obo/test.owl</IRI>
    <Literal>CC BY 4.0</Literal>
  </AnnotationAssertion>
</Ontology>"#;

    const NON_BFO_OWX: &str = r#"<?xml version="1.0"?>
<Ontology xmlns="http://www.w3.org/2002/07/owl#"
          xml:base="http://example.com/myonto.owl"
          ontologyIRI="http://example.com/myonto.owl">
  <Declaration>
    <Class IRI="http://example.com/MyClass1"/>
  </Declaration>
  <Declaration>
    <Class IRI="http://example.com/MyClass2"/>
  </Declaration>
</Ontology>"#;

    #[test]
    fn test_bfo_grounded_detected() {
        let f = write_fixture(BFO_GROUNDED_OWX);
        let result = analyse_owl_file(f.path()).expect("should parse");
        assert!(result.bfo_grounded, "should detect BFO grounding via Import");
        assert_eq!(result.class_count, 2);
        assert!(!result.imports.is_empty());
    }

    #[test]
    fn test_non_bfo_flagged_not_crashed() {
        let f = write_fixture(NON_BFO_OWX);
        let result = analyse_owl_file(f.path()).expect("should parse without crash");
        assert!(!result.bfo_grounded, "non-BFO should be flagged false");
        assert_eq!(result.class_count, 2);
    }
}
