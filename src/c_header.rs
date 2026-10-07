use std::path::Path;

#[derive(Clone, Debug)]
pub(crate) struct ParsedHeader {
    pub(crate) typedefs: Vec<TypedefDef>,
    pub(crate) structs: Vec<StructDef>,
    pub(crate) enums: Vec<EnumDef>,
    pub(crate) functions: Vec<FunctionDef>,
}

#[derive(Clone, Debug)]
pub(crate) struct TypedefDef {
    pub(crate) name: String,
    pub(crate) target: String,
}

#[derive(Clone, Debug)]
pub(crate) struct StructDef {
    pub(crate) name: String,
    pub(crate) fields: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub(crate) struct EnumDef {
    pub(crate) name: String,
    pub(crate) variants: Vec<(String, Option<String>)>,
}

#[derive(Clone, Debug)]
pub(crate) struct FunctionDef {
    pub(crate) name: String,
    pub(crate) return_type: String,
    pub(crate) params: Vec<(String, String)>,
}

/// Drop `/* … */` and `//` comments from C source text.
///
/// Generated and hand-written headers both annotate declarations
/// (`/* enrichment */ uint32_t crc32fast_hash(const uint8_t *data, size_t len);`),
/// and a comment left in the signature text becomes part of the type. Block
/// comments may span lines; a line comment keeps its newline so the line
/// structure a caller parses is unchanged.
pub(crate) fn strip_c_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '/' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('*') => {
                chars.next();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    if c == '*' && matches!(chars.peek(), Some('/')) {
                        chars.next();
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    // Unterminated: the rest of the text is comment.
                    break;
                }
            }
            Some('/') => {
                chars.next();
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

pub(crate) fn parse_c_header(content: &str) -> ParsedHeader {
    const MAX_TYPEDEF_BLOCK_LINES: usize = 16_384;

    let mut typedefs = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut functions = Vec::new();

    let mut i = 0;
    let stripped = strip_c_comments(content);
    let lines: Vec<&str> = stripped.lines().map(str::trim).collect();

    while i < lines.len() {
        let line = lines[i];

        if line.starts_with("typedef") {
            if line.contains("enum") && line.contains('{') {
                let mut enum_content = String::new();
                let mut extend_lines = 0usize;
                while i < lines.len() && !lines[i].contains('}') {
                    extend_lines += 1;
                    if extend_lines > MAX_TYPEDEF_BLOCK_LINES {
                        break;
                    }
                    enum_content.push_str(lines[i]);
                    enum_content.push(' ');
                    i += 1;
                }
                if i < lines.len() && lines[i].contains('}') {
                    enum_content.push_str(lines[i]);
                }
                if let Some(parsed) = parse_typedef_enum(&enum_content) {
                    typedefs.push(TypedefDef {
                        name: parsed.name.clone(),
                        target: format!("enum {}", parsed.name),
                    });
                    enums.push(parsed);
                }
            } else if line.contains("struct") && line.contains('{') {
                let mut struct_content = String::new();
                let mut extend_lines = 0usize;
                while i < lines.len() && !lines[i].contains('}') {
                    extend_lines += 1;
                    if extend_lines > MAX_TYPEDEF_BLOCK_LINES {
                        break;
                    }
                    struct_content.push_str(lines[i]);
                    struct_content.push(' ');
                    i += 1;
                }
                if i < lines.len() && lines[i].contains('}') {
                    struct_content.push_str(lines[i]);
                }
                if let Some(parsed) = parse_typedef_struct(&struct_content) {
                    typedefs.push(TypedefDef {
                        name: parsed.name.clone(),
                        target: format!("struct {}", parsed.name),
                    });
                    structs.push(parsed);
                }
            } else if line.ends_with(';') {
                if let Some((target, name)) = parse_typedef_line(line) {
                    typedefs.push(TypedefDef { name, target });
                }
            }
        }

        if !line.starts_with("typedef")
            && !line.starts_with("struct")
            && !line.starts_with("enum")
            && !line.starts_with("//")
            && !line.starts_with("#")
            && line.contains('(')
            && (line.ends_with(';') || line.ends_with('{'))
        {
            if let Some(func) = parse_function_line(line) {
                functions.push(func);
            }
        }

        i += 1;
    }

    ParsedHeader {
        typedefs,
        structs,
        enums,
        functions,
    }
}

pub(crate) fn parse_typedef_struct(content: &str) -> Option<StructDef> {
    let content = content.trim();
    let end_part = content.strip_suffix(';')?.trim();
    let name = end_part.split_whitespace().last()?.to_string();
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    let fields_str = &content[start + 1..end];

    let mut fields = Vec::new();
    for field in fields_str.split(';') {
        let field = field.trim();
        if field.is_empty() || field.starts_with("//") {
            continue;
        }

        if let Some((field_type, field_name)) = parse_c_field(field) {
            fields.push((field_type, field_name));
        }
    }

    Some(StructDef { name, fields })
}

/// Split a C field/declarator into (type, name), moving pointer stars into the type and folding
/// trailing array dimensions into the type (`uint8_t bytes[256]` -> ("uint8_t[256]", "bytes"),
/// `const char *name` -> ("const char *", "name")). Returns None for declarators whose name is not
/// a plain identifier (function pointers, bitfields) — those are handled elsewhere or skipped.
pub(crate) fn parse_c_field(decl: &str) -> Option<(String, String)> {
    let decl = decl.trim();
    let (head, dims) = match split_array_dims(decl) {
        Some((h, d)) => (h, d),
        None => (decl.to_string(), Vec::new()),
    };
    let head = head.trim();
    // The name is the trailing identifier run; everything before it (incl. `*`) is the type.
    let name_start = head
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map(|i| i + 1)
        .unwrap_or(0);
    let name = &head[name_start..];
    if !is_c_identifier(name) {
        return None;
    }
    let base_type = head[..name_start].trim();
    if base_type.is_empty() {
        return None;
    }
    let mut type_str = base_type.to_string();
    for dim in &dims {
        type_str.push('[');
        type_str.push_str(dim);
        type_str.push(']');
    }
    Some((type_str, name.to_string()))
}

pub(crate) fn parse_typedef_enum(content: &str) -> Option<EnumDef> {
    let content = content.trim();
    let end_part = content.strip_suffix(';')?.trim();
    let name = end_part.split_whitespace().last()?.to_string();
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    let variants_str = &content[start + 1..end];

    let mut variants = Vec::new();
    for item in variants_str.split(',') {
        let item = item.trim();
        if item.is_empty() || item.starts_with("//") {
            continue;
        }

        if let Some((name, value)) = item.split_once('=') {
            variants.push((name.trim().to_string(), Some(value.trim().to_string())));
        } else {
            variants.push((item.to_string(), None));
        }
    }

    Some(EnumDef { name, variants })
}

pub(crate) fn parse_typedef_line(line: &str) -> Option<(String, String)> {
    let line = strip_c_comments(line);
    let line = line.strip_prefix("typedef")?.trim();
    let line = line.strip_suffix(';')?.trim();

    let parts: Vec<&str> = line.rsplitn(2, ' ').collect();
    if parts.len() == 2 {
        Some((parts[1].to_string(), parts[0].to_string()))
    } else {
        None
    }
}

pub(crate) fn parse_function_line(line: &str) -> Option<FunctionDef> {
    let line = strip_c_comments(line);
    let line = line
        .strip_suffix(';')
        .or_else(|| line.strip_suffix('{'))?
        .trim();

    let paren_start = line.find('(')?;
    let paren_end = line.rfind(')')?;

    let signature = &line[..paren_start].trim();
    let params_str = &line[paren_start + 1..paren_end];

    let parts: Vec<&str> = signature.rsplitn(2, ' ').collect();
    let (mut return_type, mut name) = if parts.len() == 2 {
        (parts[1].to_string(), parts[0].to_string())
    } else {
        ("void".to_string(), parts[0].to_string())
    };
    // `const char *label(void);` splits as ("const char", "*label") — hoist the
    // stars onto the return type so the name stays a C identifier.
    if name.starts_with('*') {
        let stars: String = name.chars().take_while(|&c| c == '*').collect();
        name = name[stars.len()..].to_string();
        return_type = format!("{return_type} {stars}");
    }

    let params: Vec<(String, String)> = if params_str.trim() == "void" || params_str.is_empty() {
        Vec::new()
    } else {
        params_str
            .split(',')
            .filter_map(|p| {
                let p = p.trim();
                let parts: Vec<&str> = p.rsplitn(2, ' ').collect();
                if parts.len() == 2 {
                    let (mut typ, mut name) = (parts[1].to_string(), parts[0].to_string());
                    if name.starts_with('*') {
                        let stars: String = name.chars().take_while(|&c| c == '*').collect();
                        name = name[stars.len()..].to_string();
                        typ = format!("{} {}", typ, stars);
                    }
                    Some((typ, name))
                } else {
                    None
                }
            })
            .collect()
    };

    Some(FunctionDef {
        name,
        return_type,
        params,
    })
}

/// Remove qualifiers that carry no Rust/ABI meaning (`volatile`, `restrict` and its spellings)
/// wherever they appear in a C type, so `volatile int *` or `const restrict float *` map to the
/// right pointer type instead of collapsing to the `*mut c_void` catch-all.
pub(crate) fn strip_ignored_qualifiers(c_type: &str) -> String {
    c_type
        .split_whitespace()
        .filter(|tok| {
            !matches!(
                *tok,
                "volatile" | "restrict" | "__restrict" | "__restrict__"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Peel trailing C array dimensions off a type, e.g. `int [4][4]` -> ("int", ["4","4"]). Returns
/// None when there is no array suffix.
pub(crate) fn split_array_dims(c_type: &str) -> Option<(String, Vec<String>)> {
    let mut cur = c_type.trim();
    if !cur.ends_with(']') {
        return None;
    }
    let mut dims = Vec::new();
    while cur.ends_with(']') {
        let open = cur.rfind('[')?;
        dims.push(cur[open + 1..cur.len() - 1].trim().to_string());
        cur = cur[..open].trim_end();
    }
    dims.reverse();
    Some((cur.trim().to_string(), dims))
}

pub(crate) fn c_type_to_rust(c_type: &str) -> String {
    let normalized = strip_ignored_qualifiers(c_type);
    let c_type = normalized.trim();

    // Fixed-size arrays map to Rust arrays, nesting for multiple dimensions (`int[4][4]` is an
    // array of 4 rows of 4). An empty dimension (`[]`, a flexible member) becomes length 0.
    if let Some((base, dims)) = split_array_dims(c_type) {
        let mut rust = c_type_to_rust(&base);
        for dim in dims.iter().rev() {
            let size = if dim.is_empty() { "0" } else { dim.as_str() };
            rust = format!("[{rust}; {size}]");
        }
        return rust;
    }

    match c_type {
        "void" => "()".to_string(),
        "int" => "c_int".to_string(),
        "unsigned int" | "uint" => "c_uint".to_string(),
        "long" => "c_long".to_string(),
        "unsigned long" | "ulong" => "c_ulong".to_string(),
        "long long" => "c_longlong".to_string(),
        "unsigned long long" => "c_ulonglong".to_string(),
        "short" => "c_short".to_string(),
        "unsigned short" | "ushort" => "c_ushort".to_string(),
        "char" => "c_char".to_string(),
        "unsigned char" | "uchar" => "c_uchar".to_string(),
        "signed char" => "c_schar".to_string(),
        "float" => "c_float".to_string(),
        "double" => "c_double".to_string(),
        "size_t" => "usize".to_string(),
        "ssize_t" => "isize".to_string(),
        "bool" | "_Bool" => "bool".to_string(),
        "uint8_t" => "u8".to_string(),
        "uint16_t" => "u16".to_string(),
        "uint32_t" => "u32".to_string(),
        "uint64_t" => "u64".to_string(),
        "int8_t" => "i8".to_string(),
        "int16_t" => "i16".to_string(),
        "int32_t" => "i32".to_string(),
        "int64_t" => "i64".to_string(),
        s if s.ends_with('*') => {
            let inner = s.strip_suffix('*').unwrap().trim();
            if inner == "void" {
                "*mut c_void".to_string()
            } else if inner == "const void" {
                "*const c_void".to_string()
            } else if let Some(pointee) = inner.strip_prefix("const ") {
                let pointee = pointee.trim();
                if pointee.ends_with('*') {
                    // `const T **` is a mutable pointer to a pointer to const
                    // T: the const belongs to the inner pointer, not the outer.
                    format!("*mut {}", c_type_to_rust(inner))
                } else {
                    format!("*const {}", c_type_to_rust(pointee))
                }
            } else {
                format!("*mut {}", c_type_to_rust(inner))
            }
        }
        s if s.starts_with("const ") => c_type_to_rust(s.strip_prefix("const ").unwrap()),
        other if is_c_identifier(other) => other.to_string(),
        _ => "*mut c_void".to_string(),
    }
}

pub(crate) fn c_type_to_rust_checked(c_type: &str) -> Result<String, String> {
    let mapped = c_type_to_rust(c_type);
    let trimmed = c_type.trim();
    if mapped == "*mut c_void"
        && trimmed != "void *"
        && trimmed != "void*"
        && !trimmed.ends_with("void *")
        && !trimmed.ends_with("void*")
        && !is_known_or_ident_type(trimmed)
    {
        return Err(format!("unsupported C type `{trimmed}`"));
    }
    Ok(mapped)
}

fn is_known_or_ident_type(c_type: &str) -> bool {
    let c_type = c_type.trim();
    if c_type.ends_with('*') {
        return is_known_or_ident_type(c_type.strip_suffix('*').unwrap().trim());
    }
    if let Some(inner) = c_type.strip_prefix("const ") {
        return is_known_or_ident_type(inner);
    }
    is_c_abi_safe_scalar(c_type) || is_c_identifier(c_type)
}

pub(crate) fn is_c_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(crate) fn rust_ident(name: &str) -> Option<String> {
    if !is_c_identifier(name) {
        return None;
    }
    if is_rust_keyword(name) {
        Some(format!("r#{name}"))
    } else {
        Some(name.to_string())
    }
}

fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
            | "gen"
    )
}

pub(crate) fn parse_enum_discriminant(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        return i64::from_str_radix(hex, 16).ok();
    }
    if value.starts_with('+') || value.starts_with('-') {
        return value.parse().ok();
    }
    if value.bytes().all(|b| b.is_ascii_digit()) {
        return value.parse().ok();
    }
    None
}

pub(crate) fn is_c_abi_safe_type(c_type: &str) -> bool {
    let c_type = c_type.trim();
    if c_type.is_empty() {
        return false;
    }
    if c_type.starts_with("struct ") || c_type.starts_with("enum ") {
        return false;
    }
    if let Some(inner) = c_type.strip_suffix('*') {
        let inner = inner.trim();
        let inner = inner.strip_prefix("const ").unwrap_or(inner).trim();
        if inner == "void" {
            return true;
        }
        return is_c_abi_safe_type(inner);
    }
    is_c_abi_safe_scalar(c_type)
}

fn is_c_abi_safe_scalar(c_type: &str) -> bool {
    matches!(
        c_type,
        "void"
            | "int"
            | "unsigned int"
            | "uint"
            | "long"
            | "unsigned long"
            | "ulong"
            | "long long"
            | "unsigned long long"
            | "short"
            | "unsigned short"
            | "ushort"
            | "char"
            | "unsigned char"
            | "uchar"
            | "signed char"
            | "float"
            | "double"
            | "size_t"
            | "ssize_t"
            | "bool"
            | "_Bool"
            | "uint8_t"
            | "uint16_t"
            | "uint32_t"
            | "uint64_t"
            | "int8_t"
            | "int16_t"
            | "int32_t"
            | "int64_t"
    )
}

pub(crate) fn header_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("equilibrium")
        .to_string()
}

#[cfg(test)]
mod type_mapping_tests {
    use super::c_type_to_rust;

    #[test]
    fn arrays_map_to_rust_arrays() {
        assert_eq!(c_type_to_rust("uint8_t[256]"), "[u8; 256]");
        assert_eq!(c_type_to_rust("int[4][4]"), "[[c_int; 4]; 4]");
        assert_eq!(c_type_to_rust("char[]"), "[c_char; 0]");
    }

    #[test]
    fn struct_fields_keep_arrays_and_pointers() {
        use super::parse_typedef_struct;
        let s = parse_typedef_struct(
            "typedef struct Buffer { uint8_t bytes[256]; size_t len; const char *name; } Buffer;",
        )
        .expect("parsed struct");
        assert_eq!(s.name, "Buffer");
        assert_eq!(
            s.fields,
            vec![
                ("uint8_t[256]".to_string(), "bytes".to_string()),
                ("size_t".to_string(), "len".to_string()),
                ("const char *".to_string(), "name".to_string()),
            ]
        );
    }

    #[test]
    fn qualifiers_do_not_collapse_pointers_to_void() {
        // volatile/restrict carry no Rust ABI meaning and must not derail the mapping.
        assert_eq!(c_type_to_rust("volatile int *"), "*mut c_int");
        assert_eq!(c_type_to_rust("const restrict float *"), "*const c_float");
        assert_eq!(c_type_to_rust("int * restrict"), "*mut c_int");
        assert_eq!(c_type_to_rust("volatile uint8_t"), "u8");
        // unqualified mappings are unchanged.
        assert_eq!(c_type_to_rust("const char *"), "*const c_char");
        assert_eq!(c_type_to_rust("void *"), "*mut c_void");
    }
}
