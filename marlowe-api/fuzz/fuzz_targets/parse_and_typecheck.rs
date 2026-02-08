#![no_main]

use libfuzzer_sys::fuzz_target;
use marlowe_api::{parse_contract_yaml, type_check, TypeCheckContext};

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        if let Ok(contract) = parse_contract_yaml(input) {
            let _ = type_check(&contract, &TypeCheckContext::default());
        }
    }
});
