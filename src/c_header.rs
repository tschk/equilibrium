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

pub(crate) fn parse_c_header(content: &str) -> ParsedHeader {
    const MAX_TYPEDEF_BLOCK_LINES: usize = 16_384;

    let mut typedefs = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut functions = Vec::new();

    let mut i = 0;
    let lines: Vec<&str> = content.lines().collect();

    while i < lines.len() {
        let line = lines[i].trim();

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

        let parts: Vec<&str> = field.split_whitespace().collect();
        if parts.len() >= 2 {
            let field_name = parts.last().unwrap().trim_end_matches('[').to_string();
            let field_type = parts[..parts.len() - 1].join(" ");
            fields.push((field_type, field_name));
        }
    }

    Some(StructDef { name, fields })
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
    let line = line
        .strip_suffix(';')
        .or_else(|| line.strip_suffix('{'))?
        .trim();

    let paren_start = line.find('(')?;
    let paren_end = line.rfind(')')?;

    let signature = &line[..paren_start].trim();
    let params_str = &line[paren_start + 1..paren_end];

    let parts: Vec<&str> = signature.rsplitn(2, ' ').collect();
    let (return_type, name) = if parts.len() == 2 {
        (parts[1].to_string(), parts[0].to_string())
    } else {
        ("void".to_string(), parts[0].to_string())
    };

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

pub(crate) fn c_type_to_rust(c_type: &str) -> String {
    let c_type = c_type.trim();

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
            } else if inner.starts_with("const ") {
                let inner_type = c_type_to_rust(inner.strip_prefix("const ").unwrap());
                format!("*const {}", inner_type)
            } else {
                format!("*mut {}", c_type_to_rust(inner))
            }
        }
        s if s.starts_with("const ") => c_type_to_rust(s.strip_prefix("const ").unwrap()),
        other => other.to_string(),
    }
}

pub(crate) fn is_c_abi_safe_type(c_type: &str) -> bool {
    let c_type = c_type.trim();
    if c_type.is_empty() {
        return false;
    }
    if c_type.starts_with("struct ") || c_type.starts_with("enum ") {
        return false;
    }
    if c_type.ends_with('*') {
        return true;
    }
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
