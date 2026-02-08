use serde_json::Value;

#[test]
fn openapi_document_contains_simulation_endpoints() {
    let json = marlowe_api::http::openapi_json().expect("openapi generation");
    let doc: Value = serde_json::from_str(&json).expect("valid json");

    assert!(doc["paths"]["/simulate/step"].is_object());
    assert!(doc["paths"]["/simulate/preview"].is_object());
    assert!(doc["paths"]["/health"].is_object());

    assert!(doc["components"]["schemas"]["SimulateStepRequest"].is_object());
    assert!(doc["components"]["schemas"]["SimulatePreviewRequest"].is_object());
    assert!(doc["components"]["schemas"]["SimulateErrorResponse"].is_object());
}
