#![no_main]

use libfuzzer_sys::fuzz_target;
use marlowe_api::parse_contract_yaml;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let _ = parse_contract_yaml(input);
    }
});
