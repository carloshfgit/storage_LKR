use std::path::Path;

use crate::model::Category;

/// Categoria de um caminho.
///
/// Por enquanto só olha a extensão; as regras de caminho (sistema/apps, Lixeira,
/// ocultos do `$HOME`) entram na Etapa 2.
pub fn classify(path: &Path, _home: &Path) -> Category {
    by_extension(path).unwrap_or(Category::Other)
}

fn by_extension(path: &Path) -> Option<Category> {
    let ext = path.extension()?.to_str()?;
    // Extensões têm no máximo 4 caracteres na tabela; evita alocar para as longas.
    if ext.len() > 4 {
        return None;
    }
    let mut buf = [0u8; 4];
    let lower = &mut buf[..ext.len()];
    lower.copy_from_slice(ext.as_bytes());
    lower.make_ascii_lowercase();

    let category = match &*lower {
        b"jpg" | b"jpeg" | b"png" | b"gif" | b"webp" | b"heic" | b"heif" | b"avif" | b"bmp"
        | b"tif" | b"tiff" | b"svg" | b"raw" | b"dng" | b"cr2" | b"cr3" | b"nef" | b"arw"
        | b"orf" | b"rw2" => Category::Photos,
        b"mp4" | b"m4v" | b"mkv" | b"mov" | b"avi" | b"webm" | b"wmv" | b"flv" | b"mpg"
        | b"mpeg" | b"3gp" | b"ts" => Category::Videos,
        b"mp3" | b"flac" | b"ogg" | b"oga" | b"opus" | b"m4a" | b"aac" | b"wav" | b"aiff"
        | b"wma" => Category::Audio,
        b"pdf" | b"doc" | b"docx" | b"odt" | b"rtf" | b"txt" | b"md" | b"xls" | b"xlsx"
        | b"ods" | b"csv" | b"ppt" | b"pptx" | b"odp" | b"epub" => Category::Documents,
        _ => return None,
    };
    Some(category)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(p: &str) -> Category {
        classify(Path::new(p), Path::new("/home/u"))
    }

    #[test]
    fn extension_is_case_insensitive() {
        assert_eq!(cat("/home/u/Vídeos/Férias.MKV"), Category::Videos);
        assert_eq!(cat("/home/u/a.JpEg"), Category::Photos);
    }

    #[test]
    fn unknown_or_missing_extension_is_other() {
        assert_eq!(cat("/home/u/Downloads/ubuntu.iso"), Category::Other);
        assert_eq!(cat("/home/u/Makefile"), Category::Other);
        assert_eq!(cat("/home/u/arquivo.extensaolonga"), Category::Other);
    }
}
