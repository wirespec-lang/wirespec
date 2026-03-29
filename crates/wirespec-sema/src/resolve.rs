// crates/wirespec-sema/src/resolve.rs
use std::collections::HashMap;
use crate::types::*;

/// What kind of declaration a name refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    VarInt,
    Packet,
    Frame,
    Capsule,
    Enum,
    Flags,
    StateMachine,
    Const,
}

/// Result of resolving a type name.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedType {
    Primitive(PrimitiveWireType, Option<Endianness>),
    UserDefined(String, DeclKind),
}

/// Registry of all known type names in a module.
pub struct TypeRegistry {
    module_endianness: Endianness,
    /// user-defined name → DeclKind
    declarations: HashMap<String, DeclKind>,
    /// alias name → target name (resolved transitively)
    aliases: HashMap<String, String>,
    /// const name → value (for compile-time evaluation)
    const_values: HashMap<String, i64>,
}

impl TypeRegistry {
    pub fn new(module_endianness: Endianness) -> Self {
        Self {
            module_endianness,
            declarations: HashMap::new(),
            aliases: HashMap::new(),
            const_values: HashMap::new(),
        }
    }

    /// Register an externally-imported type (from another module).
    /// External types go into the same declarations map.
    /// They won't conflict with local declarations because the
    /// driver processes modules in dependency order.
    pub fn register_external(&mut self, name: &str, kind: DeclKind) {
        self.declarations.insert(name.to_string(), kind);
    }

    pub fn register(&mut self, name: &str, kind: DeclKind) -> Result<(), String> {
        if self.declarations.contains_key(name) {
            return Err(format!("duplicate definition: '{name}'"));
        }
        self.declarations.insert(name.to_string(), kind);
        Ok(())
    }

    pub fn register_alias(&mut self, alias: &str, target: &str) {
        self.aliases.insert(alias.to_string(), target.to_string());
    }

    pub fn register_const(&mut self, name: &str, value: i64) {
        self.const_values.insert(name.to_string(), value);
    }

    pub fn get_const_value(&self, name: &str) -> Option<i64> {
        self.const_values.get(name).copied()
    }

    pub fn module_endianness(&self) -> Endianness {
        self.module_endianness
    }

    pub fn contains(&self, name: &str) -> bool {
        self.declarations.contains_key(name)
            || self.aliases.contains_key(name)
            || self.resolve_primitive(name).is_some()
    }

    pub fn get_decl_kind(&self, name: &str) -> Option<DeclKind> {
        self.declarations.get(name).copied()
    }

    /// Resolve a type name to its meaning.
    pub fn resolve_type_name(&self, name: &str) -> Option<ResolvedType> {
        self.resolve_type_name_inner(name, 0)
    }

    fn resolve_type_name_inner(&self, name: &str, depth: usize) -> Option<ResolvedType> {
        if depth > 32 {
            return None; // cycle guard
        }

        // Check alias first (transitive)
        if let Some(target) = self.aliases.get(name) {
            return self.resolve_type_name_inner(target, depth + 1);
        }

        // Check primitives
        if let Some((prim, endian)) = self.resolve_primitive(name) {
            return Some(ResolvedType::Primitive(prim, endian));
        }

        // Check user declarations
        if let Some(kind) = self.declarations.get(name) {
            return Some(ResolvedType::UserDefined(name.to_string(), *kind));
        }

        None
    }

    /// Return all known type names (declarations, aliases, and primitives).
    pub fn all_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.declarations.keys().cloned().collect();
        names.extend(self.aliases.keys().cloned());
        // Add primitive names
        for prim in &[
            "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "u16be", "u16le", "u24",
            "u24be", "u24le", "u32be", "u32le", "u64be", "u64le", "i16be", "i16le", "i32be",
            "i32le", "i64be", "i64le", "bool", "bit",
        ] {
            names.push(prim.to_string());
        }
        names
    }

    fn resolve_primitive(&self, name: &str) -> Option<(PrimitiveWireType, Option<Endianness>)> {
        let (prim, endian) = match name {
            "u8" => (PrimitiveWireType::U8, None),
            "u16" => (PrimitiveWireType::U16, Some(self.module_endianness)),
            "u24" => (PrimitiveWireType::U24, Some(self.module_endianness)),
            "u32" => (PrimitiveWireType::U32, Some(self.module_endianness)),
            "u64" => (PrimitiveWireType::U64, Some(self.module_endianness)),
            "i8" => (PrimitiveWireType::I8, None),
            "i16" => (PrimitiveWireType::I16, Some(self.module_endianness)),
            "i32" => (PrimitiveWireType::I32, Some(self.module_endianness)),
            "i64" => (PrimitiveWireType::I64, Some(self.module_endianness)),
            "u16be" => (PrimitiveWireType::U16, Some(Endianness::Big)),
            "u16le" => (PrimitiveWireType::U16, Some(Endianness::Little)),
            "u24be" => (PrimitiveWireType::U24, Some(Endianness::Big)),
            "u24le" => (PrimitiveWireType::U24, Some(Endianness::Little)),
            "u32be" => (PrimitiveWireType::U32, Some(Endianness::Big)),
            "u32le" => (PrimitiveWireType::U32, Some(Endianness::Little)),
            "u64be" => (PrimitiveWireType::U64, Some(Endianness::Big)),
            "u64le" => (PrimitiveWireType::U64, Some(Endianness::Little)),
            "i16be" => (PrimitiveWireType::I16, Some(Endianness::Big)),
            "i16le" => (PrimitiveWireType::I16, Some(Endianness::Little)),
            "i32be" => (PrimitiveWireType::I32, Some(Endianness::Big)),
            "i32le" => (PrimitiveWireType::I32, Some(Endianness::Little)),
            "i64be" => (PrimitiveWireType::I64, Some(Endianness::Big)),
            "i64le" => (PrimitiveWireType::I64, Some(Endianness::Little)),
            "bool" => (PrimitiveWireType::Bool, None),
            "bit" => (PrimitiveWireType::Bit, None),
            _ => return None,
        };
        Some((prim, endian))
    }
}
