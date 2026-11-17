//! Port of `ALGoal.java` (record) plus the `Identifier` seam.

/// Upstream seam: `net.minecraft.resources.Identifier` (namespace:path).
/// Parsing validates the shape; the value compares by string, exactly as
/// upstream `Identifier.equals` compares components.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Identifier(pub String);

#[derive(Debug, PartialEq)]
pub struct InvalidIdentifier;

impl Identifier {
    pub fn parse(s: &str) -> Result<Identifier, InvalidIdentifier> {
        // Upstream validatePath/validateNamespace: [a-z0-9_.-]+ on both sides
        // of a single ':', namespace optional (defaults to "minecraft").
        let (ns, path) = match s.split_once(':') {
            Some((ns, path)) => (ns, path),
            None => ("minecraft", s),
        };
        let valid = |s: &str| {
            !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.' || c == '-')
        };
        if valid(ns) && valid(path) {
            Ok(Identifier(s.to_string()))
        } else {
            Err(InvalidIdentifier)
        }
    }
}

/// Upstream `record ALGoal(Identifier enchant, int lvlMin, int lvlMax, int priceMin, int priceMax)`.
/// `enchant` is nullable upstream (checked with `if(enc_id == null) continue`),
/// so it is `Option<Identifier>` here.
#[derive(Clone, Debug, PartialEq)]
pub struct ALGoal {
    pub enchant: Option<Identifier>,
    pub lvl_min: i32,
    pub lvl_max: i32,
    pub price_min: i32,
    pub price_max: i32,
}
