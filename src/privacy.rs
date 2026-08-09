// SPDX-License-Identifier: MPL-2.0

pub const PRIVATE_VALUE: &str = "••••";

pub fn redact(value: &str, hide: bool) -> &str {
    if hide && !value.is_empty() {
        PRIVATE_VALUE
    } else {
        value
    }
}
