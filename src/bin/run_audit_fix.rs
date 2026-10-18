use lopdf::{Document, Object};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use windows::{
    core::HSTRING,
    Data::Pdf::{PdfDocument, PdfPage, PdfPageRenderOptions},
    Graphics::Imaging::{
        BitmapDecoder, BitmapRotation, BitmapTransform, ColorManagementMode, ExifOrientationMode,
    },
    Media::Ocr::OcrEngine,
    Storage::StorageFile,
    Storage::Streams::InMemoryRandomAccessStream,
};

// ==================== PENGATURAN UTAMA ====================
const ROOT_FOLDER: &str = r"G:\My Drive\notaris bu sri\Pihak Kedua";

// Daftar 10 nama file dokumen standar notaris
const NAMA_SUDAH_BENAR: [&str; 10] = [
    "ajb.pdf",
    "bphtb.pdf",
    "cek.pdf",
    "keabsahan.pdf",
    "kk.pdf",
    "ktp.pdf",
    "pbb.pdf",
    "pph.pdf",
    "permohonan.pdf",
    "sertifikat.pdf",
];

/// Memeriksa apakah nama file sudah standar notaris (termasuk KTP & KK perorangan)
fn is_nama_file_standar(file_name: &str) -> bool {
    let lower = file_name.to_lowercase();
    if lower == "ktp penjual.pdf" || lower.starts_with("ktp penjual_")
        || lower == "ktp persetujuan.pdf" || lower.starts_with("ktp persetujuan_")
        || lower == "ktp pembeli.pdf" || lower.starts_with("ktp pembeli_")
        || lower == "kk penjual.pdf" || lower.starts_with("kk penjual_")
        || lower == "kk pembeli.pdf" || lower.starts_with("kk pembeli_")
    {
        return true;
    }

    NAMA_SUDAH_BENAR.iter().any(|&b| {
        let stem = b.trim_end_matches(".pdf");
        lower == b || lower.starts_with(&format!("{}_", stem))
    })
}

/// Memeriksa apakah folder dikecualikan:
/// Folder dikecualikan jika namanya tidak berhuruf kapital semua (memuat huruf kecil seperti 'finish', 'old data', 'revisi', dll.)
fn is_folder_dikecualikan(path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(ROOT_FOLDER) else {
        return false;
    };

    for comp in rel.components() {
        let name = comp.as_os_str().to_string_lossy();
        if name.to_lowercase().ends_with(".pdf") {
            continue;
        }

        if name.chars().any(|c| c.is_lowercase()) {
            return true;
        }
    }

    false
}

/// Mencocokkan teks hasil OCR dengan 10 jenis dokumen standar notaris
fn tentukan_jenis_dokumen(teks_mentah: &str) -> Option<&'static str> {
    let t = teks_mentah.to_uppercase();

    // 1. Keabsahan.pdf
    if t.contains("KEABSAHAN DAN KEBENARAN")
        || t.contains("PERTANGGUNGJAWABAN KEABSAHAN")
        || t.contains("NAMA PENGGUNA TERDAFTAR")
        || (t.contains("SURAT PERNYATAAN") && t.contains("KEABSAHAN"))
    {
        return Some("Keabsahan");
    }

    // 2. Permohonan.pdf (Formulir permohonan ke Kepala Kantor Pertanahan)
    if (t.contains("KEPALA KANTOR PERTANAHAN") || t.contains("KANTOR PERTANAHAN"))
        && (t.contains("MENGAJUKAN PERMOHONAN")
            || t.contains("PENDAFTARAN PERALIHAN HAK")
            || t.contains("UNTUK MELENGKAPI PERMOHONAN")
            || t.contains("SURAT PERMOHONAN"))
    {
        return Some("Permohonan");
    }

    // 3. Cek.pdf (Hasil Pengecekan Sertipikat BPN)
    if t.contains("PENGECEKAN SERTIPIKAT")
        || t.contains("DAFTAR HAK TANGGUNGAN")
        || t.contains("TIDAK SEDANG DIAGUNKAN")
        || t.contains("TIDAK TERDAPAT BLOKIR")
        || t.contains("HASIL PENGECEKAN")
    {
        return Some("Cek");
    }

    // 4. PPH.pdf (Surat penelitian formal, bukti setor, billing DJP, dan bukti bayar/NTPN)
    if t.contains("PENELITIAN BUKTI PEMENUHAN")
        || t.contains("PAJAK PENGHASILAN")
        || t.contains("411128")
        || t.contains("PHTB-CT")
        || (t.contains("SURAT SETORAN PAJAK") && t.contains("PPH"))
        || (t.contains("KODE BILLING") && (t.contains("PAJAK") || t.contains("NTPN") || t.contains("DJP")))
        || (t.contains("NTPN") && (t.contains("PAJAK") || t.contains("SETORAN")))
    {
        return Some("PPH");
    }

    // 5. PBB.pdf (Mencakup SPPT PBB maupun Surat Keterangan NJOP)
    if t.contains("SPPT PBB")
        || t.contains("PEMBERITAHUAN PAJAK TERHUTANG")
        || t.contains("PAJAK BUMI DAN BANGUNAN")
        || t.contains("411312")
        || t.contains("BUKAN MERUPAKAN BUKTI KEPEMILIKAN")
        || t.contains("SURAT KETERANGAN NJOP")
        || t.contains("SISMIOP")
    {
        return Some("PBB");
    }

    // 6. BPHTB.pdf (Surat Setoran Pajak Daerah Bea Perolehan Hak Atas Tanah)
    if t.contains("SSPD")
        || t.contains("SURAT SETORAN PAJAK DAERAH")
        || t.contains("BEA PEROLEHAN HAK")
        || t.contains("NPOPTKP")
        || t.contains("BPHTB")
        || (t.contains("PAJAK DAERAH") && (t.contains("BEA") || t.contains("HAK") || t.contains("SETORAN") || t.contains("SETU-AN")))
    {
        return Some("BPHTB");
    }

    // 7. KK.pdf (Kartu Keluarga)
    if t.contains("KARTU KELUARGA")
        || (t.contains("KARTU") && (t.contains("KEI-UARGA") || t.contains("KEIUARGA") || t.contains("KE1UARGA") || t.contains("KEPALA")))
        || t.contains("NAMA KEPALA KELUARGA")
        || t.contains("KEPALA KELUARGA")
        || ((t.contains("KEPALA") || t.contains("PENCATATAN SIPIL") || t.contains("KEPENDUDUKAN"))
            && (t.contains("RT/RW") || t.contains("ALAMAT"))
            && (t.contains("AYAH") || t.contains("IBU") || t.contains("HUBUNGAN") || t.contains("KELUARGA")))
    {
        return Some("KK");
    }

    // 8. AJB.pdf (Akta Jual Beli otentik PPAT)
    if !t.contains("KEMENTERIAN AGRARIA") && !t.contains("BADAN PERTANAHAN") && !t.contains("EDISI") {
        if t.contains("AKTA JUAL BELI")
            || t.contains("AKTA GUAL BELI")
            || (t.contains("AKTA") && t.contains("BELI") && (t.contains("PPAT") || t.contains("PEJABAT PEMBUAT AKTA TANAH")))
        {
            return Some("AJB");
        }
    }

    // 9. Sertifikat.pdf (Sertipikat Hak Tanah BPN)
    let is_sertifikat = (t.contains("SERTIPIKAT") || t.contains("SERTIFIKAT") || t.contains("EDISI"))
        && (t.contains("KEMENTERIAN AGRARIA")
            || t.contains("BADAN PERTANAHAN")
            || t.contains("PERTANAHAN")
            || t.contains("AGRARIA")
            || t.contains("DAFTAR ISIAN")
            || t.contains("SURAT UKUR")
            || t.contains("BUKU TANAH")
            || t.contains("LETAK BIDANG")
            || t.contains("PEMEGANG HAK")
            || t.contains("TANDA BUKTI HAK")
            || t.contains("HAK MILIK")
            || t.contains("HAK GUNA BANGUNAN")
            || t.contains("HAK PAKAI")
            || t.contains("ROYA")
            || t.contains("PEMECAHAN")
            || t.contains("PENATAAN BATAS"))
        || (t.contains("KEMENTERIAN AGRARIA") && t.contains("BADAN PERTANAHAN"))
        || (t.contains("PEMEGANG HAK") && t.contains("BIDANG TANAH"))
        || (t.contains("BUKU TANAH") && (t.contains("HAK MILIK") || t.contains("HAK GUNA BANGUNAN")))
        || (t.contains("DAFTAR ISIAN 206") || t.contains("DAFTAR ISIAN 208"));

    if is_sertifikat
        && !t.contains("KARTU TANDA PENDUDUK")
        && !t.contains("PENGECEKAN SERTIPIKAT")
        && !t.contains("HASIL PENGECEKAN")
    {
        return Some("Sertifikat");
    }

    // 10. KTP.pdf (KTP biasa, e-KTP, atau fotokopi KTP)
    let is_ktp = t.contains("KARTU TANDA PENDUDUK")
        || t.contains("SEUMUR")
        || t.contains("SEIJMUR")
        || t.contains("SEUUUR")
        || (t.contains("NIK") && (t.contains("33") || t.contains("PROVINSI") || t.contains("KABUPATEN") || t.contains("KOTA") || t.contains("AGAMA") || t.contains("KAWIN") || t.contains("WARGA")))
        || ((t.contains("PROVINSI") || t.contains("PROV")) && (t.contains("KABUPATEN") || t.contains("PATI") || t.contains("DEMAK") || t.contains("GROBOGAN") || t.contains("SEMARANG") || t.contains("REMBANG") || t.contains("JAWA TENGAH") || t.contains("BANTEN") || t.contains("LAMPUNG")))
        || ((t.contains("JENIS KELAMIN")
            || t.contains("STATUS PERKAWINAN")
            || t.contains("KEWARGANEGARAAN")
            || t.contains("TEMPAT/TGL LAHIR")
            || t.contains("GOL. DARAH")
            || t.contains("LAHIR")
            || t.contains("AGAMA")
            || t.contains("PEKERJAAN"))
            && (t.contains("NIK") || t.contains("PROVINSI") || t.contains("KABUPATEN") || t.contains("INDONESIA") || t.contains("WNI") || t.contains("WNE") || t.contains("DEMAK") || t.contains("SEMARANG") || t.contains("REMBANG") || t.contains("PATI") || t.contains("HIBUP") || t.contains("KAWIN")))
        || ((t.contains("FOTO SESUAI") || t.contains("FOTO SE.SUA") || t.contains("FOTO COPY SESUAI") || t.contains("FOTO COPY") || t.contains("DIPERLIHATKAN") || t.contains("PERLIHATKAN") || t.contains("3374") || t.contains("3321") || t.contains("3318"))
            && (t.contains("NIK") || t.contains("LAHIR") || t.contains("WARGA") || t.contains("AGAMA") || t.contains("NOTARIS") || t.contains("KEPADASAYA") || t.contains("KEPADA SAYA") || t.contains("ASLI")));

    if is_ktp {
        return Some("KTP");
    }

    None
}

/// Menjalankan OCR pada satu halaman PDF dengan rotasi multi-arah (0°, 270°, 90°, 180°)
/// Mengembalikan: (Option<jenis_dokumen>, rotasi_agar_tegak_dalam_derajat, teks_lengkap_halaman)
fn deteksi_dan_orientasi_halaman(page: &PdfPage, engine: &OcrEngine) -> (Option<&'static str>, u32, String) {
    let Some(size) = page.Size().ok() else { return (None, 0, String::new()); };
    let scale: f32 = if size.Width > 1000.0 || size.Height > 1000.0 { 1.5 } else { 2.2 };
    let Ok(render_options) = PdfPageRenderOptions::new() else { return (None, 0, String::new()); };
    let _ = render_options.SetDestinationWidth((size.Width * scale) as u32);
    let _ = render_options.SetDestinationHeight((size.Height * scale) as u32);

    let Ok(stream) = InMemoryRandomAccessStream::new() else { return (None, 0, String::new()); };
    let Ok(render_op) = page.RenderWithOptionsToStreamAsync(&stream, &render_options) else { return (None, 0, String::new()); };
    if render_op.get().is_err() { return (None, 0, String::new()); }

    let Ok(decoder_op) = BitmapDecoder::CreateAsync(&stream) else { return (None, 0, String::new()); };
    let Ok(decoder) = decoder_op.get() else { return (None, 0, String::new()); };

    let rotations = [
        (0u32, BitmapRotation::None),
        (270u32, BitmapRotation::Clockwise270Degrees),
        (90u32, BitmapRotation::Clockwise90Degrees),
        (180u32, BitmapRotation::Clockwise180Degrees),
    ];

    let mut best_kategori: Option<&'static str> = None;
    let mut best_rot: u32 = 0;
    let mut max_word_count: usize = 0;
    let mut all_text = String::new();

    for (deg, rot) in rotations {
        let Ok(transform) = BitmapTransform::new() else { continue; };
        let _ = transform.SetRotation(rot);

        let Ok(pixel_format) = decoder.BitmapPixelFormat() else { continue; };
        let Ok(alpha_mode) = decoder.BitmapAlphaMode() else { continue; };

        let Ok(bitmap_op) = decoder.GetSoftwareBitmapTransformedAsync(
            pixel_format,
            alpha_mode,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        ) else { continue; };
        let Ok(software_bitmap) = bitmap_op.get() else { continue; };

        let Ok(ocr_op) = engine.RecognizeAsync(&software_bitmap) else { continue; };
        let Ok(ocr_result) = ocr_op.get() else { continue; };
        let text = ocr_result.Text().map(|h| h.to_string()).unwrap_or_default();
        all_text.push_str(" ");
        all_text.push_str(&text);

        let kat_opt = tentukan_jenis_dokumen(&text);
        let word_count = text.split_whitespace().filter(|w| w.len() >= 2).count();

        if let Some(kat) = kat_opt {
            if best_kategori.is_none() {
                best_kategori = Some(kat);
                best_rot = deg;
                max_word_count = word_count;
            }
            if deg == 0 {
                return (Some(kat), 0, all_text);
            }
        }

        if best_kategori.is_none() && word_count > max_word_count {
            max_word_count = word_count;
            best_rot = deg;
        }

        if deg == 0 && word_count >= 30 {
            break;
        }
    }

    if best_kategori.is_none() {
        best_kategori = tentukan_jenis_dokumen(&all_text);
    }

    (best_kategori, best_rot, all_text)
}

/// Menjalankan OCR langsung pada halaman tertentu suatu berkas PDF
fn ocr_halaman_langsung(path: &Path, page_idx: u32, engine: &OcrEngine) -> String {
    let Some(path_str) = path.to_str() else { return String::new(); };
    let h_path = HSTRING::from(path_str);
    let Ok(file_op) = StorageFile::GetFileFromPathAsync(&h_path) else { return String::new(); };
    let Ok(file) = file_op.get() else { return String::new(); };
    let Ok(pdf_doc_op) = PdfDocument::LoadFromFileAsync(&file) else { return String::new(); };
    let Ok(pdf_doc) = pdf_doc_op.get() else { return String::new(); };

    let total = pdf_doc.PageCount().unwrap_or(0);
    if page_idx >= total {
        return String::new();
    }

    let Ok(page) = pdf_doc.GetPage(page_idx) else { return String::new(); };
    deteksi_dan_orientasi_halaman(&page, engine).2
}

/// Meluruskan seluruh halaman PDF yang miring agar posisinya tegak (upright)
fn luruskan_halaman_pdf(
    path: &Path,
    rotations: &[(u32, u32)],
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let ada_rotasi = rotations.iter().any(|&(_, rot)| rot != 0);
    if !ada_rotasi {
        return Ok(false);
    }

    let mut doc = Document::load(path)?;
    let pages = doc.get_pages();

    let mut diubah = false;
    for &(p_num, additional_rot) in rotations {
        if additional_rot == 0 {
            continue;
        }

        if let Some(&page_id) = pages.get(&p_num) {
            if let Ok(page_dict) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
                let current_rot = page_dict
                    .get(b"Rotate")
                    .ok()
                    .and_then(|o| o.as_i64().ok())
                    .unwrap_or(0);
                let new_rot = (current_rot + additional_rot as i64) % 360;
                page_dict.set("Rotate", Object::Integer(new_rot));
                diubah = true;
            }
        }
    }

    if diubah {
        let temp_path = path.with_extension("tmp_rot.pdf");
        doc.save(&temp_path)?;
        fs::rename(&temp_path, path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Mengekstrak sejumlah halaman tertentu dari PDF asal ke PDF tujuan baru secara lossless
fn extract_pages(source_pdf: &Path, target_pdf: &Path, pages_to_keep: &[u32]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let src_str = source_pdf.to_str().ok_or("Invalid source path")?;
    let dst_str = target_pdf.to_str().ok_or("Invalid target path")?;

    // Coba ekstraksi menggunakan PyMuPDF via Python
    let py_code = "import pymupdf, sys\n\
src = sys.argv[1]\n\
dst = sys.argv[2]\n\
keep = [int(x) - 1 for x in sys.argv[3:]]\n\
doc = pymupdf.open(src)\n\
n = pymupdf.open()\n\
for p in keep:\n\
    if 0 <= p < len(doc):\n\
        n.insert_pdf(doc, from_page=p, to_page=p)\n\
n.save(dst)\n\
n.close()\n\
doc.close()\n";

    let mut cmd = std::process::Command::new("python");
    cmd.args(["-c", py_code, src_str, dst_str]);
    for p in pages_to_keep {
        cmd.arg(p.to_string());
    }

    if let Ok(out) = cmd.output() {
        if out.status.success() && target_pdf.exists() {
            if let Ok(meta) = fs::metadata(target_pdf) {
                if meta.len() > 500 {
                    return Ok(());
                }
            }
        }
    }

    // Fallback native jika Python tidak tersedia
    let mut doc = Document::load(source_pdf)?;
    let all_pages: Vec<u32> = doc.get_pages().keys().cloned().collect();
    let keep_set: HashSet<u32> = pages_to_keep.iter().cloned().collect();
    let to_delete: Vec<u32> = all_pages.into_iter().filter(|p| !keep_set.contains(p)).collect();

    doc.delete_pages(&to_delete);
    doc.save(target_pdf)?;
    Ok(())
}


/// Hasil analisis isi berkas PDF
#[derive(Debug)]
enum HasilAnalisisPdf {
    TidakDikenali(Vec<(u32, u32)>),
    Tunggal(&'static str, Vec<(u32, u32)>, String),
    Bundel(Vec<(&'static str, Vec<u32>)>, Vec<(u32, u32)>),
}

/// Menganalisis berkas PDF: apakah berkas tunggal atau bundel multi-dokumen
fn analisis_pdf(path: &Path, engine: &OcrEngine) -> HasilAnalisisPdf {
    let Some(path_str) = path.to_str() else { return HasilAnalisisPdf::TidakDikenali(Vec::new()); };
    let h_path = HSTRING::from(path_str);
    let Ok(file_op) = StorageFile::GetFileFromPathAsync(&h_path) else { return HasilAnalisisPdf::TidakDikenali(Vec::new()); };
    let Ok(file) = file_op.get() else { return HasilAnalisisPdf::TidakDikenali(Vec::new()); };
    let Ok(pdf_doc_op) = PdfDocument::LoadFromFileAsync(&file) else { return HasilAnalisisPdf::TidakDikenali(Vec::new()); };
    let Ok(pdf_doc) = pdf_doc_op.get() else { return HasilAnalisisPdf::TidakDikenali(Vec::new()); };

    let total_pages = pdf_doc.PageCount().unwrap_or(0);
    if total_pages == 0 {
        return HasilAnalisisPdf::TidakDikenali(Vec::new());
    }

    let mut page_results: Vec<(u32, Option<&'static str>, u32, String)> = Vec::new();
    let mut distinct_categories: HashSet<&'static str> = HashSet::new();
    let mut page_rotations: Vec<(u32, u32)> = Vec::new();
    let mut full_doc_text = String::new();

    for p_idx in 0..total_pages {
        let page_num = p_idx + 1;
        let (kat_opt, rot, p_text) = if let Ok(page) = pdf_doc.GetPage(p_idx) {
            deteksi_dan_orientasi_halaman(&page, engine)
        } else {
            (None, 0, String::new())
        };

        if let Some(k) = kat_opt {
            distinct_categories.insert(k);
        }
        full_doc_text.push_str(" ");
        full_doc_text.push_str(&p_text);
        page_results.push((page_num, kat_opt, rot, p_text));
        page_rotations.push((page_num, rot));
    }

    if total_pages == 1 {
        if let Some(kat) = page_results[0].1 {
            return HasilAnalisisPdf::Tunggal(kat, page_rotations, full_doc_text);
        }
        return HasilAnalisisPdf::TidakDikenali(page_rotations);
    }

    // AJB adalah akta otentik PPAT utuh yang tidak boleh dipisah
    if page_results.get(0).and_then(|p| p.1) == Some("AJB")
        || page_results.get(1).and_then(|p| p.1) == Some("AJB")
    {
        return HasilAnalisisPdf::Tunggal("AJB", page_rotations, full_doc_text);
    }

    // Sertifikat adalah buku/sertipikat hak tanah BPN utuh yang tidak boleh dipisah
    if page_results.get(0).and_then(|p| p.1) == Some("Sertifikat")
        || page_results.get(1).and_then(|p| p.1) == Some("Sertifikat")
    {
        return HasilAnalisisPdf::Tunggal("Sertifikat", page_rotations, full_doc_text);
    }

    if distinct_categories.is_empty() {
        return HasilAnalisisPdf::TidakDikenali(page_rotations);
    }

    // Jika hanya ada 1 jenis dokumen di seluruh halaman (misal: Sertifikat 2 halaman atau PBB 2 halaman)
    if distinct_categories.len() == 1 {
        let jenis = distinct_categories.into_iter().next().unwrap();
        // KTP dan KK adalah dokumen lembar tunggal perorangan/keluarga; jika multi-halaman, harus dipisah!
        if jenis != "KTP" && jenis != "KK" {
            return HasilAnalisisPdf::Tunggal(jenis, page_rotations, full_doc_text);
        }
    }

    // Jika memuat lebih dari 1 jenis dokumen atau multi-halaman KTP/KK -> BUNDEL MULTI-DOKUMEN
    let mut groups: Vec<(&'static str, Vec<u32>)> = Vec::new();
    for (page_num, kat_opt, _, _) in page_results {
        if let Some(kat) = kat_opt {
            // KTP dan KK tidak boleh digabung ke halaman sebelumnya yang sejenis (tiap lembar adalah dokumen tersendiri)
            if kat != "KTP" && kat != "KK" {
                if let Some(last_group) = groups.last_mut() {
                    if last_group.0 == kat {
                        last_group.1.push(page_num);
                        continue;
                    }
                }
            }
            groups.push((kat, vec![page_num]));
        } else {
            // Halaman tidak memiliki kata kunci baru (lampiran/bukti transaksi/lembar lanjutan)
            // Gabungkan ke dokumen sebelumnya yang sedang aktif, KECUALI jika dokumen sebelumnya adalah KTP
            if let Some(last_group) = groups.last_mut() {
                if last_group.0 != "KTP" {
                    last_group.1.push(page_num);
                    continue;
                }
            }
            groups.push(("Dokumen", vec![page_num]));
        }
    }

    if groups.len() <= 1 {
        if let Some((jenis, _)) = groups.first() {
            return HasilAnalisisPdf::Tunggal(jenis, page_rotations, full_doc_text);
        }
        return HasilAnalisisPdf::TidakDikenali(page_rotations);
    }

    HasilAnalisisPdf::Bundel(groups, page_rotations)
}

fn catat_log(pesan: &str) {
    println!("{}", pesan);
    let log_path = Path::new(r"C:\Users\ryokh\Data\Coding\auto-rename-notary-file\auto_rename.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_path) {
        use std::io::Write;
        let _ = writeln!(f, "{}", pesan);
    }
}

/// Memisahkan berkas bundel menjadi berkas individual di folder yang sama serta meluruskan halamannya
fn pisahkan_bundel_pdf(
    path: &Path,
    groups: &[(&'static str, Vec<u32>)],
    rotations: &[(u32, u32)],
) -> bool {
    let Some(parent_dir) = path.parent() else { return false; };
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();

    catat_log(&format!(
        "     [BERKAS BUNDEL TERDETEKSI] Berkas '{}' memuat {} dokumen berbeda. Memulai pemisahan berkas...",
        file_name, groups.len()
    ));

    let mut target_names_taken: HashSet<String> = HashSet::new();
    let mut prepared_items: Vec<(PathBuf, PathBuf, &'static str, Vec<u32>, bool)> = Vec::new();
    let mut semua_sukses = true;

    for (jenis, pages) in groups {
        let mut target_name = format!("{}.pdf", jenis);
        let mut counter = 2;
        loop {
            let potential_path = parent_dir.join(&target_name);
            let conflicts_with_other = potential_path.exists() && potential_path != path;
            let already_taken = target_names_taken.contains(&target_name.to_lowercase());
            if !conflicts_with_other && !already_taken {
                break;
            }
            target_name = format!("{}_{}.pdf", jenis, counter);
            counter += 1;
        }

        target_names_taken.insert(target_name.to_lowercase());
        let final_target_path = parent_dir.join(&target_name);

        let temp_name = format!("_temp_split_{}_{}_{}.pdf", jenis, std::process::id(), pages[0]);
        let temp_path = parent_dir.join(&temp_name);

        if let Err(e) = extract_pages(path, &temp_path, pages) {
            catat_log(&format!("       [Gagal Ekstrak] Dokumen {} halaman {:?}: {}", jenis, pages, e));
            semua_sukses = false;
            let _ = fs::remove_file(&temp_path);
            break;
        }

        let group_rotations: Vec<(u32, u32)> = pages
            .iter()
            .enumerate()
            .map(|(new_idx, orig_page)| {
                let rot = rotations
                    .iter()
                    .find(|(p, _)| p == orig_page)
                    .map(|(_, r)| *r)
                    .unwrap_or(0);
                (new_idx as u32 + 1, rot)
            })
            .collect();

        let diluruskan = luruskan_halaman_pdf(&temp_path, &group_rotations).unwrap_or(false);

        prepared_items.push((temp_path, final_target_path, *jenis, pages.clone(), diluruskan));
    }

    if !semua_sukses {
        // Hapus file temporary yang sempat dibuat
        for (temp_path, _, _, _, _) in prepared_items {
            let _ = fs::remove_file(temp_path);
        }
        catat_log("     -> Pemisahan berkas bundel belum lengkap. Berkas asal tetap dipertahankan.");
        return false;
    }

    // Seluruh potongan berhasil diekstrak ke temp_path!
    // Sekarang bersihkan/hapus berkas bundel asal SEBELUM memindahkan file hasil potongan ke nama target
    // sehingga jika salah satu target memiliki nama yang sama dengan bundel asal, file tersebut tidak terhapus.
    if let Err(e) = fs::remove_file(path) {
        catat_log(&format!("       [Peringatan] Gagal menghapus berkas bundel asal '{}': {}", file_name, e));
    } else {
        catat_log(&format!("     -> SUKSES! Seluruh {} dokumen berhasil dipisahkan dan berkas bundel '{}' telah dibersihkan.", groups.len(), file_name));
    }

    for (temp_path, final_target_path, jenis, pages, diluruskan) in prepared_items {
        let final_name = final_target_path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if let Err(e) = fs::rename(&temp_path, &final_target_path) {
            catat_log(&format!("       [Gagal Simpan] {:?} -> {}: {}", temp_path.file_name().unwrap(), final_name, e));
            let _ = fs::remove_file(&temp_path);
        } else if diluruskan {
            catat_log(&format!(
                "       * Berhasil dipisahkan & diluruskan: '{}' (halaman {:?}) -> {}",
                jenis, pages, final_name
            ));
        } else {
            catat_log(&format!(
                "       * Berhasil dipisahkan: '{}' (halaman {:?}) -> {}",
                jenis, pages, final_name
            ));
        }
    }

    true
}

// ==================== STRUKTUR DATA UNTUK AJB, KTP & KK ====================
#[derive(Debug, Clone, Default)]
struct PihakData {
    nama_lengkap: Vec<String>,
    nama_kata: Vec<String>,
    nik_list: Vec<String>,
    alamat_kata: Vec<String>,
    is_perempuan: bool,
}

#[derive(Debug)]
enum StatusPenjual {
    Pt,
    Perorangan {
        penjual: PihakData,
        persetujuan: PihakData,
        pembeli: PihakData,
    },
    TidakDikenali,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum KtpRole {
    Penjual,
    Persetujuan,
    Pembeli,
    Standar,
}

impl KtpRole {
    fn file_prefix(&self) -> &'static str {
        match self {
            KtpRole::Penjual => "KTP PENJUAL",
            KtpRole::Persetujuan => "KTP PERSETUJUAN",
            KtpRole::Pembeli => "KTP PEMBELI",
            KtpRole::Standar => "KTP",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum KkRole {
    Pembeli,
    Penjual,
    #[allow(dead_code)]
    Standar,
}

impl KkRole {
    fn file_prefix(&self) -> &'static str {
        match self {
            KkRole::Pembeli => "KK PEMBELI",
            KkRole::Penjual => "KK PENJUAL",
            KkRole::Standar => "KK",
        }
    }
}

/// Mengekstrak kata-kata unik dari bagian alamat dalam AJB (Kelurahan, Desa, Kecamatan, Kabupaten, dsb.)
fn extract_address_words_section(sec: &str) -> Vec<String> {
    let mut words = Vec::new();
    let stop = [
        "BERTEMPAT", "TINGGAL", "RUKUN", "TETANGGA", "WARGA", "KELURAHAN", "DESA",
        "KECAMATAN", "KABUPATEN", "KOTA", "PROVINSI", "JAWA", "TENGAH", "PADA", "SAAT",
        "INI", "BERADA", "KEDUA", "DUANYA",
    ];
    let upper = sec.to_uppercase();
    if let Some(pos) = upper.find("BERTEMPAT TINGGAL DI") {
        let sub = &upper[pos + 20..];
        let end_pos = sub.find('.').unwrap_or_else(|| sub.find(',').unwrap_or(120.min(sub.len())));
        for w in sub[..end_pos].split_whitespace() {
            let clean: String = w.chars().filter(|c| c.is_alphabetic()).collect();
            if clean.len() >= 4 && !stop.contains(&clean.as_str()) && !words.contains(&clean) {
                words.push(clean);
            }
        }
    }
    words
}

/// Mengekstrak NIK (angka 16 digit atau digit berurutan >= 12 digit)
fn extract_nik_from_str(text: &str) -> Vec<String> {
    let mut res = Vec::new();
    let digits: String = text
        .chars()
        .map(|c| if c.is_ascii_digit() { c } else { ' ' })
        .collect();
    for token in digits.split_whitespace() {
        if token.len() >= 12 && token.len() <= 18 {
            if !res.contains(&token.to_string()) {
                res.push(token.to_string());
            }
        }
    }
    res
}

/// Mengekstrak nama lengkap setelah sebutan Tuan/Nyonya/Nona/Tn./Ny. serta kata-kata uniknya
fn extract_names_after_honorifics(text: &str) -> (Vec<String>, Vec<String>) {
    let mut full_names = Vec::new();
    let mut name_words = Vec::new();

    let stop_words = [
        "TUAN", "NYONYA", "NONA", "TN", "NY", "WARGA", "NEGARA", "INDONESIA", "LAHIR",
        "TANGGAL", "TAHUN", "KARYAWAN", "SWASTA", "WIRASWASTA", "PEMEGANG", "NOMOR",
        "INDUK", "KEPENDUDUKAN", "BERTEMPAT", "TINGGAL", "RUKUN", "TETANGGA", "KELURAHAN",
        "KECAMATAN", "KABUPATEN", "KOTA", "MENGURUS", "RUMAH", "TANGGA", "BELUM", "TIDAK",
        "BEKERJA", "PEGAWAI", "NEGERI", "SIPIL", "MENURUT", "KETERANGANNYA", "UNTUK",
        "MELAKUKAN", "PERBUATAN", "HUKUM", "DALAM", "AKTA", "TELAH", "MENDAPAT", "PERSETUJUAN",
        "SUAMINYA", "ISTRINYA", "PENGHADAP", "YANG", "TURUT", "HADIR", "MENGHADAP", "KEPADA",
        "SAYA", "PPAT", "SERTA", "PARA", "SAKSI", "SELANJUTNYA", "DISEBUT", "PIHAK", "PERTAMA",
        "PENJUAL", "KEDUA", "PEMBELI", "SRI", "WAHYUNINGSIH",
    ];

    let upper = text.to_uppercase();
    let markers = ["TUAN ", "NYONYA ", "NONA ", "TN. ", "NY. ", "TUAN-", "NYONYA-", "TN-", "NY-"];

    for marker in &markers {
        let mut curr_pos = 0;
        while let Some(idx) = upper[curr_pos..].find(marker) {
            let actual_idx = curr_pos + idx + marker.len();
            let sub = &text[actual_idx..];
            let mut name_part = String::new();
            for c in sub.chars() {
                if c == ',' || c == '.' || c == '(' || c == ';' || c == '\n' || c == '\r' {
                    break;
                }
                name_part.push(c);
            }
            let cleaned = name_part
                .replace("Warga", "")
                .replace("WARGA", "")
                .replace("Negara", "")
                .replace("NEGARA", "")
                .replace("Indonesia", "")
                .replace("INDONESIA", "")
                .trim()
                .to_uppercase();

            let letters_only: String = cleaned.chars().filter(|c| c.is_alphabetic() || c.is_whitespace()).collect();
            let trimmed = letters_only.trim().to_string();
            if trimmed.len() >= 3 && !trimmed.starts_with("PPAT") && !trimmed.starts_with("SRI WAHYUNINGSIH") {
                if !full_names.contains(&trimmed) {
                    for w in trimmed.split_whitespace() {
                        if w.len() >= 3 && !stop_words.contains(&w) {
                            if !name_words.contains(&w.to_string()) {
                                name_words.push(w.to_string());
                            }
                        }
                    }
                    full_names.push(trimmed);
                }
            }
            curr_pos = actual_idx;
        }
    }
    (full_names, name_words)
}

/// Menganalisis teks AJB untuk membedakan apakah penjual adalah PT atau Perorangan
fn parse_ajb_text(text: &str) -> StatusPenjual {
    let upper = text.to_uppercase();

    let idx_pihak_pertama = upper
        .find("PIHAK PERTAMA")
        .or_else(|| upper.find("PENJUAL"))
        .unwrap_or(upper.len() / 2);

    let pihak_pertama_section = if idx_pihak_pertama < upper.len() {
        &upper[..idx_pihak_pertama]
    } else {
        &upper[..]
    };

    let pt_in_p1 = pihak_pertama_section.contains("PERSEROAN TERBATAS")
        || pihak_pertama_section.contains("DIREKTUR")
        || pihak_pertama_section.contains("MEWAKILI PERSEROAN")
        || pihak_pertama_section.contains("KOMISARIS UTAMA")
        || pihak_pertama_section.contains("KOMISARIS PERSEROAN")
        || pihak_pertama_section.contains("PT. ")
        || (pihak_pertama_section.contains("PT.") && !pihak_pertama_section.contains("PPAT"));

    if pt_in_p1 {
        return StatusPenjual::Pt;
    }

    let idx_p2 = upper
        .find("PIHAK KEDUA")
        .or_else(|| upper.find("PEMBELI"))
        .unwrap_or(upper.len());

    let text_p1 = &text[..idx_pihak_pertama.min(text.len())];
    let text_p2 = if idx_pihak_pertama < idx_p2 && idx_pihak_pertama < text.len() {
        &text[idx_pihak_pertama..idx_p2.min(text.len())]
    } else {
        &text[idx_pihak_pertama.min(text.len())..]
    };

    let upper_p1 = text_p1.to_uppercase();
    let (penjual_str, persetujuan_str) = if let Some(pos_setuju) = upper_p1.find("PERSETUJUAN") {
        (&text_p1[..pos_setuju], &text_p1[pos_setuju..])
    } else {
        (text_p1, "")
    };

    let (penjual_full, penjual_words) = extract_names_after_honorifics(penjual_str);
    let penjual_niks = extract_nik_from_str(penjual_str);
    let penjual_addr = extract_address_words_section(penjual_str);
    let penjual_is_pr = penjual_str.to_uppercase().contains("NYONYA") || penjual_str.to_uppercase().contains("NONA");

    let (persetujuan_full, persetujuan_words) = extract_names_after_honorifics(persetujuan_str);
    let persetujuan_niks = extract_nik_from_str(persetujuan_str);
    let persetujuan_addr = if !penjual_addr.is_empty() { penjual_addr.clone() } else { extract_address_words_section(persetujuan_str) };
    let persetujuan_is_pr = persetujuan_str.to_uppercase().contains("NYONYA") || persetujuan_str.to_uppercase().contains("NONA");

    let (pembeli_full, pembeli_words) = extract_names_after_honorifics(text_p2);
    let pembeli_niks = extract_nik_from_str(text_p2);
    let pembeli_addr = extract_address_words_section(text_p2);
    let pembeli_is_pr = text_p2.to_uppercase().contains("NYONYA") || text_p2.to_uppercase().contains("NONA");

    StatusPenjual::Perorangan {
        penjual: PihakData {
            nama_lengkap: penjual_full,
            nama_kata: penjual_words,
            nik_list: penjual_niks,
            alamat_kata: penjual_addr,
            is_perempuan: penjual_is_pr,
        },
        persetujuan: PihakData {
            nama_lengkap: persetujuan_full,
            nama_kata: persetujuan_words,
            nik_list: persetujuan_niks,
            alamat_kata: persetujuan_addr,
            is_perempuan: persetujuan_is_pr,
        },
        pembeli: PihakData {
            nama_lengkap: pembeli_full,
            nama_kata: pembeli_words,
            nik_list: pembeli_niks,
            alamat_kata: pembeli_addr,
            is_perempuan: pembeli_is_pr,
        },
    }
}

/// Memeriksa status penjual AJB dari berkas fisik atau teks hasil OCR
fn cek_status_penjual_ajb(
    ajb_path: &Path,
    ajb_text_opt: Option<&str>,
    engine: &OcrEngine,
) -> StatusPenjual {
    if let Some(txt) = ajb_text_opt {
        let up = txt.to_uppercase();
        if up.contains("PIHAK PERTAMA") || up.contains("PENJUAL") || up.contains("PERSETUJUAN") || up.contains("PERSEROAN") {
            return parse_ajb_text(txt);
        }
    }

    let p2 = ocr_halaman_langsung(ajb_path, 1, engine);
    let p3 = ocr_halaman_langsung(ajb_path, 2, engine);
    let combined = format!("{}\n{}", p2, p3);
    parse_ajb_text(&combined)
}

/// Menghitung skor kecocokan antara teks KTP dengan data suatu pihak
fn score_ktp_match(
    ktp_text: &str,
    current_file_name: &str,
    info: &PihakData,
    role: KtpRole,
    is_pembeli: bool,
    folder_name: &str,
) -> usize {
    let upper_ktp = ktp_text.to_uppercase();
    let upper_file = current_file_name.to_uppercase();
    let mut score = 0;

    // Bonus nama berkas yang sudah ada
    match role {
        KtpRole::Penjual => {
            if upper_file.contains("PENJUAL") {
                score += 150;
            }
        }
        KtpRole::Persetujuan => {
            if upper_file.contains("PERSETUJUAN") {
                score += 150;
            }
        }
        KtpRole::Pembeli => {
            if upper_file.contains("PEMBELI") {
                score += 150;
            }
        }
        _ => {}
    }

    // 1. NIK Match (sangat kuat)
    for nik in &info.nik_list {
        if upper_ktp.contains(nik) {
            score += 100;
        } else if nik.len() >= 12 {
            for win in nik.as_bytes().windows(8) {
                let sub = std::str::from_utf8(win).unwrap_or("");
                if upper_ktp.contains(sub) {
                    score += 40;
                    break;
                }
            }
        }
    }

    // 2. Full Name Match
    for full in &info.nama_lengkap {
        if upper_ktp.contains(full) {
            score += 60;
        }
    }

    // 3. Name Word Match
    for w in &info.nama_kata {
        if upper_ktp.contains(w) {
            score += 30;
        }
    }

    // 4. Client Folder Name Match (khusus Pembeli)
    if is_pembeli {
        for fw in folder_name.to_uppercase().split_whitespace() {
            if fw.len() >= 3 && upper_ktp.contains(fw) {
                score += 35;
            }
        }
        if upper_file == "KTP.PDF" {
            score += 25;
        }
    }

    // 5. Alamat Match
    for aw in &info.alamat_kata {
        if upper_ktp.contains(aw) {
            score += 20;
        }
    }

    // 6. Jenis Kelamin Match
    if info.is_perempuan && (upper_ktp.contains("PEREMPUAN") || upper_ktp.contains("WANITA")) {
        score += 15;
    } else if !info.is_perempuan && upper_ktp.contains("LAKI-LAKI") {
        score += 15;
    }

    score
}

/// Menetapkan peran KTP (Penjual, Persetujuan, Pembeli) berdasarkan skor kecocokan
fn assign_ktp_roles(
    ktp_scores: &[(usize, usize, usize)], // (score_penjual, score_persetujuan, score_pembeli)
) -> Vec<KtpRole> {
    let n = ktp_scores.len();
    if n == 0 {
        return Vec::new();
    }

    if n == 1 {
        let (sp, ss, sb) = ktp_scores[0];
        let max_s = sp.max(ss).max(sb);
        if max_s < 20 {
            return vec![KtpRole::Pembeli];
        }
        if sp == max_s {
            return vec![KtpRole::Penjual];
        } else if ss == max_s {
            return vec![KtpRole::Persetujuan];
        } else {
            return vec![KtpRole::Pembeli];
        }
    }

    if n == 3 {
        // Urutan preferensi saat nilai imbang: Penjual (Pihak 1) mendahului Persetujuan (Pihak 2)
        let perms = [
            [KtpRole::Pembeli, KtpRole::Penjual, KtpRole::Persetujuan],
            [KtpRole::Penjual, KtpRole::Persetujuan, KtpRole::Pembeli],
            [KtpRole::Penjual, KtpRole::Pembeli, KtpRole::Persetujuan],
            [KtpRole::Pembeli, KtpRole::Persetujuan, KtpRole::Penjual],
            [KtpRole::Persetujuan, KtpRole::Penjual, KtpRole::Pembeli],
            [KtpRole::Persetujuan, KtpRole::Pembeli, KtpRole::Penjual],
        ];

        let mut best_score = -1i64;
        let mut best_perm = perms[0];

        for perm in perms {
            let mut score: i64 = 0;
            for i in 0..3 {
                let role = perm[i];
                let (sp, ss, sb) = ktp_scores[i];
                let s = match role {
                    KtpRole::Penjual => sp,
                    KtpRole::Persetujuan => ss,
                    KtpRole::Pembeli => sb,
                    KtpRole::Standar => 0,
                };
                score += s as i64;
            }
            if score > best_score {
                best_score = score;
                best_perm = perm;
            }
        }

        return best_perm.to_vec();
    }

    let mut assigned = vec![KtpRole::Standar; n];
    let mut used_penjual = false;
    let mut used_pembeli = false;

    for i in 0..n {
        let (sp, ss, sb) = ktp_scores[i];
        if sp >= 40 && sp > ss && sp > sb && !used_penjual {
            assigned[i] = KtpRole::Penjual;
            used_penjual = true;
        } else if sb >= 40 && sb > sp && sb > ss && !used_pembeli {
            assigned[i] = KtpRole::Pembeli;
            used_pembeli = true;
        } else if ss >= 30 && ss > sp && ss > sb {
            assigned[i] = KtpRole::Persetujuan;
        }
    }

    for i in 0..n {
        if assigned[i] == KtpRole::Standar {
            let (sp, ss, sb) = ktp_scores[i];
            if !used_penjual && (sp >= ss || used_pembeli) && (sp >= sb || used_pembeli) {
                assigned[i] = KtpRole::Penjual;
                used_penjual = true;
            } else if !used_pembeli && (sb >= ss || used_penjual) {
                assigned[i] = KtpRole::Pembeli;
                used_pembeli = true;
            } else {
                assigned[i] = KtpRole::Persetujuan;
            }
        }
    }

    assigned
}

/// Menghitung skor kecocokan antara teks KK dengan data Pembeli atau Penjual
fn score_kk_match(
    kk_text: &str,
    current_file_name: &str,
    info: &PihakData,
    extra_info: Option<&PihakData>,
    is_pembeli: bool,
    folder_name: &str,
) -> usize {
    let upper_kk = kk_text.to_uppercase();
    let upper_file = current_file_name.to_uppercase();
    let mut score = 0;

    if is_pembeli && upper_file.contains("PEMBELI") {
        score += 150;
    } else if !is_pembeli && upper_file.contains("PENJUAL") {
        score += 150;
    }

    for nik in &info.nik_list {
        if upper_kk.contains(nik) {
            score += 100;
        } else if nik.len() >= 12 {
            for win in nik.as_bytes().windows(8) {
                let sub = std::str::from_utf8(win).unwrap_or("");
                if upper_kk.contains(sub) {
                    score += 40;
                    break;
                }
            }
        }
    }

    if let Some(extra) = extra_info {
        for nik in &extra.nik_list {
            if upper_kk.contains(nik) {
                score += 80;
            } else if nik.len() >= 12 {
                for win in nik.as_bytes().windows(8) {
                    let sub = std::str::from_utf8(win).unwrap_or("");
                    if upper_kk.contains(sub) {
                        score += 30;
                        break;
                    }
                }
            }
        }
    }

    for full in &info.nama_lengkap {
        if upper_kk.contains(full) {
            score += 60;
        }
    }
    if let Some(extra) = extra_info {
        for full in &extra.nama_lengkap {
            if upper_kk.contains(full) {
                score += 50;
            }
        }
    }

    for w in &info.nama_kata {
        if upper_kk.contains(w) {
            score += 30;
        }
    }
    if let Some(extra) = extra_info {
        for w in &extra.nama_kata {
            if upper_kk.contains(w) {
                score += 25;
            }
        }
    }

    if is_pembeli {
        for fw in folder_name.to_uppercase().split_whitespace() {
            if fw.len() >= 3 && upper_kk.contains(fw) {
                score += 40;
            }
        }
    }

    for aw in &info.alamat_kata {
        if upper_kk.contains(aw) {
            score += 20;
        }
    }

    score
}

/// Menetapkan peran KK (Pembeli vs Penjual) berdasarkan skor kecocokan
fn assign_kk_roles(
    kk_scores: &[(usize, usize)], // (score_pembeli, score_penjual)
) -> Vec<KkRole> {
    let n = kk_scores.len();
    if n == 0 {
        return Vec::new();
    }

    if n == 1 {
        let (sb, sp) = kk_scores[0];
        if sp > sb && sp >= 40 {
            return vec![KkRole::Penjual];
        }
        return vec![KkRole::Pembeli];
    }

    if n == 2 {
        let (sb0, sp0) = kk_scores[0];
        let (sb1, sp1) = kk_scores[1];

        let score_a = (sb0 as i64) + (sp1 as i64);
        let score_b = (sp0 as i64) + (sb1 as i64);

        if score_a >= score_b {
            return vec![KkRole::Pembeli, KkRole::Penjual];
        } else {
            return vec![KkRole::Penjual, KkRole::Pembeli];
        }
    }

    let mut assigned = vec![KkRole::Pembeli; n];
    for (i, &(sb, sp)) in kk_scores.iter().enumerate() {
        if sp > sb {
            assigned[i] = KkRole::Penjual;
        }
    }
    assigned
}

/// Mengaudit folder klien secara terstruktur:
/// - TAHAP 1: Memeriksa dan memisahkan berkas gabungan (bundel multi-dokumen) terlebih dahulu
/// - TAHAP 2: Memeriksa, menyesuaikan nama berkas tunggal (AJB, Sertipikat, KTP PT vs Perorangan, dll) & meluruskan posisi halaman
fn audit_dan_perbaiki_satu_folder(folder: &Path, engine: &OcrEngine) -> usize {
    let folder_name = folder.file_name().unwrap_or_default().to_string_lossy();
    let Ok(entries) = fs::read_dir(folder) else { return 0; };

    let mut files_pdf = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()) == Some("pdf".to_string())
        {
            let name = path.file_name().unwrap().to_string_lossy();
            if !name.starts_with("_temp_") {
                files_pdf.push(path);
            }
        }
    }

    if files_pdf.is_empty() {
        return 0;
    }

    let mut diperbaiki = 0;

    // =========================================================================
    // TAHAP 1: MEMERIKSA & MEMISAHKAN FILE GABUNGAN (BUNDEL) TERLEBIH DAHULU
    // =========================================================================
    catat_log(&format!("\n[Memproses Folder Klien: '{}']", folder_name));
    catat_log("  -> [TAHAP 1] Mencari dan memisahkan file gabungan (bundel)...");
    let mut ada_bundel = false;

    for file_path in &files_pdf {
        let file_name = file_path.file_name().unwrap().to_string_lossy().to_string();
        let lower = file_name.to_lowercase();
        let sudah_benar = is_nama_file_standar(&file_name);

        // Berkas AJB dan Sertifikat yang sudah bernama standar tidak pernah dipisah
        let lewati = sudah_benar && (lower == "ajb.pdf" || lower == "sertifikat.pdf");
        if !lewati {
            match analisis_pdf(file_path, engine) {
                HasilAnalisisPdf::Bundel(groups, page_rot) => {
                    ada_bundel = true;
                    if pisahkan_bundel_pdf(file_path, &groups, &page_rot) {
                        diperbaiki += groups.len();
                    }
                }
                _ => {}
            }
        }
    }

    if !ada_bundel {
        catat_log("     (Tidak ada file gabungan yang perlu dipisahkan)");
    }

    // =========================================================================
    // TAHAP 2: PENGECEKAN, PENYESUAIAN NAMA BERKAS & MELURUSKAN POSISI HALAMAN
    // =========================================================================
    catat_log("  -> [TAHAP 2] Memeriksa, menyesuaikan nama berkas & meluruskan posisi halaman...");
    let Ok(entries_after) = fs::read_dir(folder) else { return diperbaiki; };
    let mut files_to_audit = Vec::new();
    for entry in entries_after.flatten() {
        let p = entry.path();
        if p.is_file()
            && p.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()) == Some("pdf".to_string())
        {
            let name = p.file_name().unwrap().to_string_lossy();
            if !name.starts_with("_temp_") {
                files_to_audit.push(p);
            }
        }
    }

    let mut hasil_deteksi: Vec<(PathBuf, String, &'static str, Vec<(u32, u32)>, String)> = Vec::new();
    for file_path in &files_to_audit {
        let file_name = file_path.file_name().unwrap().to_string_lossy().to_string();
        match analisis_pdf(file_path, engine) {
            HasilAnalisisPdf::Tunggal(jenis, page_rot, full_text) => {
                hasil_deteksi.push((file_path.clone(), file_name, jenis, page_rot, full_text));
            }
            HasilAnalisisPdf::Bundel(_, _) => {}
            HasilAnalisisPdf::TidakDikenali(page_rot) => {
                catat_log(&format!("     [{}] Berkas '{}': Kata kunci tidak dikenali (dibiarkan)", folder_name, file_name));
                if let Ok(true) = luruskan_halaman_pdf(file_path, &page_rot) {
                    catat_log(&format!("       * Posisi berkas berhasil diluruskan -> {}", file_name));
                    diperbaiki += 1;
                }
            }
        }
    }

    // Deteksi status penjual pada AJB (PT vs Perorangan)
    let ajb_entry = hasil_deteksi.iter().find(|item| item.2 == "AJB");
    let status_penjual = if let Some((ajb_path, _, _, _, ajb_text)) = ajb_entry {
        cek_status_penjual_ajb(ajb_path, Some(ajb_text), engine)
    } else if folder.join("AJB.pdf").exists() {
        cek_status_penjual_ajb(&folder.join("AJB.pdf"), None, engine)
    } else if folder.join("ajb.pdf").exists() {
        cek_status_penjual_ajb(&folder.join("ajb.pdf"), None, engine)
    } else {
        StatusPenjual::TidakDikenali
    };

    let mut jenis_counters: HashMap<String, usize> = HashMap::new();
    let mut rencana_ubah: Vec<(PathBuf, String, PathBuf, Vec<(u32, u32)>)> = Vec::new();

    match status_penjual {
        StatusPenjual::Perorangan { penjual, persetujuan, pembeli } => {
            catat_log("     [Status AJB] Penjual terdeteksi: PERORANGAN (Bukan PT). Mengaktifkan pengecekan KTP & KK (PENJUAL, PERSETUJUAN, PEMBELI)...");

            // Pisahkan file KTP, KK, dan non-khusus
            let mut ktp_items = Vec::new();
            let mut kk_items = Vec::new();
            let mut non_khusus_items = Vec::new();

            for item in hasil_deteksi {
                if item.2 == "KTP" {
                    ktp_items.push(item);
                } else if item.2 == "KK" {
                    kk_items.push(item);
                } else {
                    non_khusus_items.push(item);
                }
            }

            // Proses dokumen non-khusus secara standar
            non_khusus_items.sort_by(|a, b| {
                let a_is_exact = a.1.to_lowercase() == format!("{}.pdf", a.2.to_lowercase());
                let b_is_exact = b.1.to_lowercase() == format!("{}.pdf", b.2.to_lowercase());
                b_is_exact.cmp(&a_is_exact)
            });

            for (current_path, current_name, jenis, page_rot, _) in non_khusus_items {
                let count = jenis_counters.entry(jenis.to_string()).or_insert(0);
                *count += 1;

                let target_file_name = if *count == 1 {
                    format!("{}.pdf", jenis)
                } else {
                    format!("{}_{}.pdf", jenis, count)
                };

                let target_path = folder.join(&target_file_name);
                if current_name.to_lowercase() != target_file_name.to_lowercase() {
                    rencana_ubah.push((current_path, current_name, target_path, page_rot));
                } else {
                    if let Ok(true) = luruskan_halaman_pdf(&current_path, &page_rot) {
                        catat_log(&format!("       * Posisi berkas diluruskan -> {} (Halaman diputar agar tegak & mudah dibaca)", current_name));
                        diperbaiki += 1;
                    }
                }
            }

            // Proses berkas KK perorangan (KK PEMBELI vs KK PENJUAL)
            if !kk_items.is_empty() {
                let scores: Vec<(usize, usize)> = kk_items
                    .iter()
                    .map(|(_, name, _, _, text)| {
                        let sb = score_kk_match(text, name, &pembeli, None, true, &folder_name);
                        let sp = score_kk_match(text, name, &penjual, Some(&persetujuan), false, &folder_name);
                        (sb, sp)
                    })
                    .collect();

                let roles = assign_kk_roles(&scores);

                for (idx, (current_path, current_name, _, page_rot, _)) in kk_items.into_iter().enumerate() {
                    let role = roles[idx];
                    let prefix = role.file_prefix();
                    let count = jenis_counters.entry(prefix.to_string()).or_insert(0);
                    *count += 1;

                    let target_file_name = if *count == 1 {
                        format!("{}.pdf", prefix)
                    } else {
                        format!("{}_{}.pdf", prefix, count)
                    };

                    let (sb, sp) = scores[idx];
                    catat_log(&format!(
                        "       [Pengecekan KK] '{}' -> {} (Skor: Pembeli={}, Penjual={})",
                        current_name, target_file_name, sb, sp
                    ));

                    let target_path = folder.join(&target_file_name);
                    if current_name.to_lowercase() != target_file_name.to_lowercase() {
                        rencana_ubah.push((current_path, current_name, target_path, page_rot));
                    } else {
                        if let Ok(true) = luruskan_halaman_pdf(&current_path, &page_rot) {
                            catat_log(&format!("       * Posisi berkas diluruskan -> {} (Halaman diputar agar tegak & mudah dibaca)", current_name));
                            diperbaiki += 1;
                        }
                    }
                }
            }

            // Proses berkas KTP perorangan: hitung skor kecocokan
            if !ktp_items.is_empty() {
                let scores: Vec<(usize, usize, usize)> = ktp_items
                    .iter()
                    .map(|(_, name, _, _, text)| {
                        let sp = score_ktp_match(text, name, &penjual, KtpRole::Penjual, false, &folder_name);
                        let ss = score_ktp_match(text, name, &persetujuan, KtpRole::Persetujuan, false, &folder_name);
                        let sb = score_ktp_match(text, name, &pembeli, KtpRole::Pembeli, true, &folder_name);
                        (sp, ss, sb)
                    })
                    .collect();

                let roles = assign_ktp_roles(&scores);

                for (idx, (current_path, current_name, _, page_rot, _)) in ktp_items.into_iter().enumerate() {
                    let role = roles[idx];
                    let prefix = role.file_prefix();
                    let count = jenis_counters.entry(prefix.to_string()).or_insert(0);
                    *count += 1;

                    let target_file_name = if *count == 1 {
                        format!("{}.pdf", prefix)
                    } else {
                        format!("{}_{}.pdf", prefix, count)
                    };

                    let (sp, ss, sb) = scores[idx];
                    catat_log(&format!(
                        "       [Pengecekan KTP] '{}' -> {} (Skor: Penjual={}, Persetujuan={}, Pembeli={})",
                        current_name, target_file_name, sp, ss, sb
                    ));

                    let target_path = folder.join(&target_file_name);
                    if current_name.to_lowercase() != target_file_name.to_lowercase() {
                        rencana_ubah.push((current_path, current_name, target_path, page_rot));
                    } else {
                        if let Ok(true) = luruskan_halaman_pdf(&current_path, &page_rot) {
                            catat_log(&format!("       * Posisi berkas diluruskan -> {} (Halaman diputar agar tegak & mudah dibaca)", current_name));
                            diperbaiki += 1;
                        }
                    }
                }
            }
        }
        _ => {
            if matches!(status_penjual, StatusPenjual::Pt) {
                catat_log("     [Status AJB] Penjual terdeteksi: PT. Berkas KTP diproses standar (KTP.pdf)...");
            }

            // Prioritaskan file yang namanya sudah Jenis.pdf agar tetap menjadi Jenis.pdf
            hasil_deteksi.sort_by(|a, b| {
                let a_is_exact = a.1.to_lowercase() == format!("{}.pdf", a.2.to_lowercase());
                let b_is_exact = b.1.to_lowercase() == format!("{}.pdf", b.2.to_lowercase());
                b_is_exact.cmp(&a_is_exact)
            });

            for (current_path, current_name, jenis, page_rot, _) in hasil_deteksi {
                let count = jenis_counters.entry(jenis.to_string()).or_insert(0);
                *count += 1;

                let target_file_name = if *count == 1 {
                    format!("{}.pdf", jenis)
                } else {
                    format!("{}_{}.pdf", jenis, count)
                };

                let target_path = folder.join(&target_file_name);

                if current_name.to_lowercase() != target_file_name.to_lowercase() {
                    rencana_ubah.push((current_path, current_name, target_path, page_rot));
                } else {
                    if let Ok(true) = luruskan_halaman_pdf(&current_path, &page_rot) {
                        catat_log(&format!("       * Posisi berkas diluruskan -> {} (Halaman diputar agar tegak & mudah dibaca)", current_name));
                        diperbaiki += 1;
                    }
                }
            }
        }
    }

    if rencana_ubah.is_empty() {
        if diperbaiki == 0 {
            catat_log("     Semua nama berkas dan orientasi posisi sudah tepat.");
        }
        return diperbaiki;
    }

    catat_log(&format!("     Ditemukan {} berkas yang perlu disesuaikan namanya. Memperbaiki...", rencana_ubah.len()));

    // Fase 1: Pindahkan ke nama sementara untuk menghindari benturan nama
    let mut temp_items: Vec<(PathBuf, PathBuf, Vec<(u32, u32)>)> = Vec::new();
    for (i, (current_path, current_name, final_target, page_rot)) in rencana_ubah.into_iter().enumerate() {
        let temp_name = format!("_temp_audit_{}_{}.pdf", i, current_path.file_stem().unwrap().to_string_lossy());
        let temp_path = folder.join(&temp_name);
        if let Err(e) = fs::rename(&current_path, &temp_path) {
            catat_log(&format!("       [Gagal Temp] {} -> {}: {}", current_name, temp_name, e));
            continue;
        }
        temp_items.push((temp_path, final_target, page_rot));
    }

    // Fase 2: Pindahkan dari nama sementara ke nama target final dan luruskan posisi halaman
    for (temp_path, mut final_target, page_rot) in temp_items {
        let mut target_name = final_target.file_name().unwrap().to_string_lossy().to_string();
        let stem = final_target.file_stem().unwrap().to_string_lossy().to_string();
        let mut counter = 2;
        while final_target.exists() && final_target != temp_path {
            let alt_name = format!("{}_{}.pdf", stem, counter);
            final_target = folder.join(&alt_name);
            target_name = alt_name;
            counter += 1;
        }

        let diluruskan = luruskan_halaman_pdf(&temp_path, &page_rot).unwrap_or(false);

        match fs::rename(&temp_path, &final_target) {
            Ok(_) => {
                if diluruskan {
                    catat_log(&format!("       * Berhasil diubah & diluruskan -> {}", target_name));
                } else {
                    catat_log(&format!("       * Berhasil diubah -> {}", target_name));
                }
                diperbaiki += 1;
            }
            Err(e) => catat_log(&format!("       * Gagal mengubah ke {}: {}", target_name, e)),
        }
    }

    diperbaiki
}

/// Menjelajahi folder klien aktif secara rekursif dan menjalankan audit berkas
fn audit_folder_rekursif(
    dir: &Path,
    engine: &OcrEngine,
    total_folder: &mut usize,
    total_diperbaiki: &mut usize,
) {
    if is_folder_dikecualikan(dir) {
        return;
    }

    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut subdirs = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && !is_folder_dikecualikan(&path) {
            subdirs.push(path);
        }
    }

    if dir != Path::new(ROOT_FOLDER) {
        *total_folder += 1;
        let perbaikan = audit_dan_perbaiki_satu_folder(dir, engine);
        *total_diperbaiki += perbaikan;
    }

    for sub in subdirs {
        audit_folder_rekursif(&sub, engine, total_folder, total_diperbaiki);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let engine = match OcrEngine::TryCreateFromUserProfileLanguages() {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("Gagal menginisialisasi Windows OCR Engine: {}", e);
            return;
        }
    };

    if args.len() > 1 {
        let target = Path::new(&args[1]);
        println!("======================================================");
        println!(" AUDIT KHUSUS FOLDER: {:?}", target);
        println!("======================================================");
        let n = audit_dan_perbaiki_satu_folder(target, &engine);
        println!("======================================================");
        println!("Selesai. Total perbaikan pada folder: {}", n);
        println!("======================================================");
        return;
    }

    println!("======================================================");
    println!(" AUDIT & PERBAIKAN NAMA DOKUMEN NOTARIS + SPLIT BUNDEL + AUTO-DESKEW");
    println!(" Target Folder : {}", ROOT_FOLDER);
    println!(" Pengecualian  : Folder huruf kecil (finish, revisi, dll)");
    println!(" Fitur         : Split Bundel | Auto-Rename (PT & Perorangan) | Auto-Deskew");
    println!("======================================================");

    let mut total_folder = 0;
    let mut total_diperbaiki = 0;
    let root = Path::new(ROOT_FOLDER);

    audit_folder_rekursif(root, &engine, &mut total_folder, &mut total_diperbaiki);

    println!("======================================================");
    println!("Total {} folder klien diperiksa.", total_folder);
    println!("Total {} berkas diperbaiki / bundel dipisahkan / diluruskan.", total_diperbaiki);
    println!("Semua berkas pada folder klien aktif telah rapi dan tegak!");
    println!("======================================================");
}
