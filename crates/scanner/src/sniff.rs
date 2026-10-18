//! Classificação pelo conteúdo (magic bytes) para arquivos sem extensão. Só roda para
//! quem cairia na regra 7 sem extensão (ARCHITECTURE §5), então são poucas leituras.

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

use rustix::fs::{Mode, OFlags};

use crate::model::Category;

/// Abaixo disso o arquivo raramente é mídia, e a leitura custa caro proporcionalmente.
pub const MIN_SNIFF_BYTES: u64 = 4096;

/// Cabeçalho lido: cobre todas as assinaturas da tabela.
const HEADER_LEN: usize = 32;

/// Lê o início de `path` e procura uma assinatura conhecida. Qualquer erro (sem
/// permissão, arquivo sumiu) vira `None`: o arquivo fica em Outros.
pub fn sniff(path: &Path) -> Option<Category> {
    let mut file = open_noatime(path)?;
    let mut buf = [0u8; HEADER_LEN];
    let mut len = 0;
    while len < buf.len() {
        match file.read(&mut buf[len..]) {
            Ok(0) => break,
            Ok(n) => len += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    from_header(&buf[..len])
}

/// `O_NOATIME` evita que a análise mude o horário de acesso dos arquivos. O kernel só
/// aceita a flag para o dono do arquivo; nos outros casos, abre sem ela.
fn open_noatime(path: &Path) -> Option<File> {
    let flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOCTTY;
    let fd = rustix::fs::open(path, flags | OFlags::NOATIME, Mode::empty())
        .or_else(|e| match e {
            rustix::io::Errno::PERM => rustix::fs::open(path, flags, Mode::empty()),
            e => Err(e),
        })
        .ok()?;
    Some(File::from(fd))
}

/// Tabela de assinaturas. ZIP (docx/odt/epub), ELF, scripts e afins ficam em Outros.
fn from_header(h: &[u8]) -> Option<Category> {
    let at = |offset: usize, sig: &[u8]| h.get(offset..offset + sig.len()) == Some(sig);

    if at(0, b"\xFF\xD8\xFF")
        || at(0, b"\x89PNG\r\n\x1A\n")
        || at(0, b"GIF87a")
        || at(0, b"GIF89a")
        // TIFF, e os RAW de câmera baseados nele (CR2, NEF, DNG, ARW).
        || at(0, b"II*\0")
        || at(0, b"MM\0*")
    {
        return Some(Category::Photos);
    }
    if at(0, b"RIFF") {
        return match h.get(8..12)? {
            b"WEBP" => Some(Category::Photos),
            b"WAVE" => Some(Category::Audio),
            b"AVI " => Some(Category::Videos),
            _ => None,
        };
    }
    // ISO base media (MP4, MOV, HEIC, M4A): `ftyp` seguido da marca principal.
    if at(4, b"ftyp") {
        return match h.get(8..12)? {
            b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"msf1" | b"avif" | b"avis" => {
                Some(Category::Photos)
            }
            b"M4A " | b"M4B " => Some(Category::Audio),
            b"isom" | b"iso2" | b"iso4" | b"iso5" | b"iso6" | b"mp41" | b"mp42" | b"avc1"
            | b"M4V " | b"qt  " | b"3gp4" | b"3gp5" | b"3gp6" | b"3g2a" | b"dash" => {
                Some(Category::Videos)
            }
            _ => None,
        };
    }
    if at(0, b"\x1A\x45\xDF\xA3") || at(0, b"\0\0\x01\xBA") {
        // Matroska/WebM e MPEG-PS (.mpg).
        return Some(Category::Videos);
    }
    if at(0, b"ID3")
        || at(0, b"fLaC")
        || at(0, b"OggS")
        || (at(0, b"FORM") && (at(8, b"AIFF") || at(8, b"AIFC")))
        // Quadro MPEG de áudio (MP3) ou ADTS (AAC), sem tag ID3. Só os bytes de sincronia
        // mais comuns, para não confundir com binários quaisquer que começam com 0xFF.
        || (h.len() >= 4
            && h[0] == 0xFF
            && matches!(h[1], 0xFB | 0xFA | 0xF3 | 0xF2 | 0xE3 | 0xF1 | 0xF9))
    {
        return Some(Category::Audio);
    }
    if at(0, b"%PDF-") {
        return Some(Category::Documents);
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::*;

    /// Assinaturas de um cabeçalho: `(offset, bytes)`.
    type Parts<'a> = &'a [(usize, &'a [u8])];

    /// Cabeçalho sintético de 32 bytes com cada assinatura no seu `offset`.
    fn header(parts: Parts) -> Vec<u8> {
        let mut h = vec![0u8; HEADER_LEN];
        for (offset, sig) in parts {
            h[*offset..offset + sig.len()].copy_from_slice(sig);
        }
        h
    }

    #[test]
    fn recognizes_signatures() {
        let cases: &[(Parts, Option<Category>)] = &[
            (&[(0, b"\xFF\xD8\xFF\xE0")], Some(Category::Photos)),
            (&[(0, b"\x89PNG\r\n\x1A\n")], Some(Category::Photos)),
            (&[(0, b"GIF89a")], Some(Category::Photos)),
            (&[(0, b"II*\0")], Some(Category::Photos)),
            (&[(0, b"RIFF"), (8, b"WEBP")], Some(Category::Photos)),
            (&[(4, b"ftypheic")], Some(Category::Photos)),
            (&[(4, b"ftypavif")], Some(Category::Photos)),
            (&[(4, b"ftypisom")], Some(Category::Videos)),
            (&[(4, b"ftypqt  ")], Some(Category::Videos)),
            (&[(0, b"\x1A\x45\xDF\xA3")], Some(Category::Videos)),
            (&[(0, b"RIFF"), (8, b"AVI ")], Some(Category::Videos)),
            (&[(4, b"ftypM4A ")], Some(Category::Audio)),
            (&[(0, b"ID3\x04")], Some(Category::Audio)),
            (&[(0, b"\xFF\xFB\x90\x64")], Some(Category::Audio)),
            (&[(0, b"fLaC")], Some(Category::Audio)),
            (&[(0, b"OggS")], Some(Category::Audio)),
            (&[(0, b"RIFF"), (8, b"WAVE")], Some(Category::Audio)),
            (&[(0, b"FORM"), (8, b"AIFF")], Some(Category::Audio)),
            (&[(0, b"%PDF-1.7")], Some(Category::Documents)),
            // Ficam em Outros: ZIP (docx/odt), ELF, script, RIFF e ftyp desconhecidos.
            (&[(0, b"PK\x03\x04")], None),
            (&[(0, b"\x7FELF")], None),
            (&[(0, b"#!/bin/sh")], None),
            (&[(0, b"RIFF"), (8, b"XXXX")], None),
            (&[(4, b"ftypxxxx")], None),
            (&[(0, b"\xFF\x00\x00\x00")], None),
        ];
        for (parts, expected) in cases {
            assert_eq!(from_header(&header(parts)), *expected, "{parts:?}");
        }
    }

    #[test]
    fn short_or_empty_headers() {
        assert_eq!(from_header(b""), None);
        assert_eq!(from_header(b"\xFF\xD8"), None);
        assert_eq!(from_header(b"RIFF\0\0\0\0WE"), None);
        // Curto, mas a assinatura inteira cabe.
        assert_eq!(from_header(b"%PDF-"), Some(Category::Documents));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("scanner-sniff-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_files_and_tolerates_errors() {
        let dir = temp_dir("files");
        let jpeg = dir.join("foto");
        let mut bytes = b"\xFF\xD8\xFF\xE0".to_vec();
        bytes.resize(8192, 0);
        fs::write(&jpeg, &bytes).unwrap();
        assert_eq!(sniff(&jpeg), Some(Category::Photos));

        let empty = dir.join("vazio");
        fs::write(&empty, b"").unwrap();
        assert_eq!(sniff(&empty), None);
        assert_eq!(sniff(&dir.join("sumiu")), None);

        // Sem permissão de leitura (root lê mesmo assim, então só confere que não quebra).
        let locked = dir.join("trancado");
        fs::write(&locked, &bytes).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let r = sniff(&locked);
        assert!(r.is_none() || r == Some(Category::Photos));

        fs::remove_dir_all(&dir).unwrap();
    }
}
