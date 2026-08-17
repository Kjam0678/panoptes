//! The values Sunrise requires. It refuses the whole file over any one of
//! them, so this runs before every save — checking each setting against the
//! same domain the page draws it from.

use serde_json::{Map, Value};

use crate::model::pointer;

use super::{
    key_bindings::validate_key_bindings,
    require_supported_schema,
    tables::{Domain, GROUPS, Setting, SettingGroup},
};

pub(crate) fn validate(document: &Value) -> Result<(), String> {
    require_supported_schema(document)?;
    let settings = document
        .pointer(pointer::ACCOUNT_SETTINGS)
        .and_then(Value::as_object)
        .ok_or("state.account.settings must be an object")?;

    for described in GROUPS {
        check_group(settings, described)?;
    }
    validate_key_bindings(settings)
}

fn check_group(settings: &Map<String, Value>, described: &SettingGroup) -> Result<(), String> {
    let values = group(settings, described.name)?;
    for setting in described.settings {
        check(values, described.name, setting)?;
    }
    Ok(())
}

/// One setting against the domain it was described with.
fn check(values: &Map<String, Value>, group_name: &str, setting: &Setting) -> Result<(), String> {
    let key = setting.key;
    match setting.domain {
        Domain::Flag => values
            .get(key)
            .and_then(Value::as_bool)
            .map(|_| ())
            .ok_or_else(|| format!("Game setting {group_name}.{key} must be true or false")),
        Domain::Choice(choices) => {
            let value = integer(values, key)?;
            choices
                .iter()
                .any(|(candidate, _)| *candidate == value)
                .then_some(())
                .ok_or_else(|| format!("Game setting {key} has an unsupported value"))
        }
        Domain::Range { minimum, maximum }
        | Domain::Offset {
            minimum, maximum, ..
        } => within(integer(values, key)?, key, minimum, maximum),
        Domain::Decimal {
            minimum, maximum, ..
        } => within(f64::from(float32(values, key)?), key, minimum, maximum),
        Domain::Exact(expected) => {
            let value = integer(values, key)?;
            if value == expected {
                Ok(())
            } else {
                Err(format!("Game setting {key} must remain {expected}"))
            }
        }
        Domain::ExactDecimal(expected) => {
            let value = float32(values, key)?;
            if value.to_bits() == expected.to_bits() {
                Ok(())
            } else {
                Err(format!("Game setting {key} must remain {expected}"))
            }
        }
    }
}

pub(super) fn group<'a>(
    settings: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a Map<String, Value>, String> {
    settings
        .get(name)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("state.account.settings.{name} must be an object"))
}

fn integer(values: &Map<String, Value>, key: &str) -> Result<u64, String> {
    values
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("Game setting {key} must be a non-negative whole number"))
}


/// Both the integer and the float settings report an out-of-range value the
/// same way, and Sunrise rejects the file over either.
fn within<T: Copy + PartialOrd + std::fmt::Display>(
    value: T,
    key: &str,
    minimum: T,
    maximum: T,
) -> Result<(), String> {
    if (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "Game setting {key} must be between {minimum} and {maximum}"
        ))
    }
}





// Sunrise stores these values as float, so validation intentionally uses the
// same f64-to-f32 conversion after serde_json parses the JSON number.
#[allow(clippy::cast_possible_truncation)]
fn float32(values: &Map<String, Value>, key: &str) -> Result<f32, String> {
    let value = values
        .get(key)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("Game setting {key} must be a number"))?;
    Ok(value)
}



#[cfg(test)]
mod tests {
    use super::*;

    /// Sunrise stores these as float, so validation has to compare the way it
    /// does: after the same f64-to-f32 narrowing.
    #[test]
    fn float_validation_matches_sunrise_float_storage() {
        let values = serde_json::json!({
            "calibration": 10000.0001,
            "ads": 1.500_000_01
        });
        let values = values.as_object().unwrap();

        let exact = Setting {
            key: "calibration",
            label: "Renderer calibration",
            domain: Domain::ExactDecimal(10_000.0),
        };
        let ranged = Setting {
            key: "ads",
            label: "ADS sensitivity modifier",
            domain: Domain::Decimal {
                minimum: 0.5,
                maximum: 1.5,
                step: 0.1,
            },
        };
        assert_eq!(check(values, "display", &exact), Ok(()));
        assert_eq!(check(values, "controls", &ranged), Ok(()));
    }

    /// The page draws a row for every setting it describes, and validation
    /// checks the same list, so a group cannot describe a setting twice.
    #[test]
    fn every_described_setting_is_named_once() {
        let mut seen = std::collections::HashSet::new();
        let mut count = 0;
        for group in GROUPS {
            for setting in group.settings {
                assert!(
                    seen.insert((group.name, setting.key)),
                    "{}.{} is described twice",
                    group.name,
                    setting.key
                );
                count += 1;
            }
        }
        assert_eq!(count, 54, "a setting was added or lost");
    }
}
