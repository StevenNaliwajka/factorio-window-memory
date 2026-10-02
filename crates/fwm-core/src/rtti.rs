//! Window classes are told apart by their MSVC RTTI type name, which is read from
//! the object's vtable at runtime (see fwm-hook). This turns that decorated name
//! into the readable key stored in positions.json.

/// `.?AVInventoryGui@@` -> `InventoryGui`, `.?AVWindow@agui@@` -> `agui::Window`.
/// Templates and other complex names keep their decorated form, which is still a
/// stable, unique key.
pub fn class_name_from_type_descriptor(raw: &str) -> Option<String> {
    let body = raw
        .strip_prefix(".?AV")
        .or_else(|| raw.strip_prefix(".?AU"))?;
    let body = body.strip_suffix("@@")?;
    if body.is_empty() {
        return None;
    }
    if body.contains(['?', '$']) {
        return Some(body.to_owned());
    }
    Some(body.split('@').rev().collect::<Vec<_>>().join("::"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_class() {
        assert_eq!(
            class_name_from_type_descriptor(".?AVInventoryGui@@").as_deref(),
            Some("InventoryGui")
        );
    }

    #[test]
    fn namespaced_class() {
        assert_eq!(
            class_name_from_type_descriptor(".?AVWindow@agui@@").as_deref(),
            Some("agui::Window")
        );
    }

    #[test]
    fn struct_prefix() {
        assert_eq!(
            class_name_from_type_descriptor(".?AUFoo@@").as_deref(),
            Some("Foo")
        );
    }

    #[test]
    fn template_keeps_its_decorated_form() {
        assert_eq!(
            class_name_from_type_descriptor(".?AV?$Dialog@W4AboutGuiResult@@@@").as_deref(),
            Some("?$Dialog@W4AboutGuiResult@@")
        );
    }

    #[test]
    fn garbage_is_rejected() {
        assert_eq!(class_name_from_type_descriptor(""), None);
        assert_eq!(class_name_from_type_descriptor("InventoryGui"), None);
        assert_eq!(class_name_from_type_descriptor(".?AV@@"), None);
    }
}
