use lopdf::Document;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
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

// Daftar 10 nama file yang sudah benar (agar tidak diproses berulang kali oleh pemantau)
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
// ==========================================================

/// Menunggu sampai file selesai ditulis oleh mesin scanner / Windows Explorer / Google Drive
async fn tunggu_file_siap(path: &Path) -> bool {
    for _ in 0..15 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if OpenOptions::new().write(true).open(path).is_ok() {
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

    if is_sertifikat && !t.contains("KARTU TANDA PENDUDUK") {
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
        || ((t.contains("FOTO SESUAI") || t.contains("FOTO SE.SUA") || t.contains("FOTO COPY SESUAI") || t.contains("DIPERLIHATKAN") || t.contains("3374") || t.contains("3321") || t.contains("3318"))
            && (t.contains("NIK") || t.contains("LAHIR") || t.contains("WARGA") || t.contains("AGAMA")));

    if is_ktp {
        return Some("KTP");
    }

    None
}

/// Menjalankan OCR pada satu halaman PDF dengan rotasi multi-arah (0°, 270°, 90°, 180°)
fn deteksi_halaman_pdf(page: &PdfPage, engine: &OcrEngine) -> Option<&'static str> {
    let size = page.Size().ok()?;
    let scale: f32 = if size.Width > 1000.0 || size.Height > 1000.0 { 1.5 } else { 2.2 };
    let render_options = PdfPageRenderOptions::new().ok()?;
    let _ = render_options.SetDestinationWidth((size.Width * scale) as u32);
    let _ = render_options.SetDestinationHeight((size.Height * scale) as u32);

    let stream = InMemoryRandomAccessStream::new().ok()?;
    let render_op = page.RenderWithOptionsToStreamAsync(&stream, &render_options).ok()?;
    render_op.get().ok()?;

    let decoder = BitmapDecoder::CreateAsync(&stream).ok()?.get().ok()?;

    let mut all_text = String::new();
    for rotation in [
        BitmapRotation::None,
        BitmapRotation::Clockwise270Degrees,
        BitmapRotation::Clockwise90Degrees,
        BitmapRotation::Clockwise180Degrees,
    ] {
        let transform = BitmapTransform::new().ok()?;
        let _ = transform.SetRotation(rotation);

        let pixel_format = decoder.BitmapPixelFormat().ok()?;
        let alpha_mode = decoder.BitmapAlphaMode().ok()?;

        let software_bitmap = decoder.GetSoftwareBitmapTransformedAsync(
            pixel_format,
            alpha_mode,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        ).ok()?.get().ok()?;

        let ocr_result = engine.RecognizeAsync(&software_bitmap).ok()?.get().ok()?;
        let text = ocr_result.Text().ok()?.to_string();
        all_text.push_str(" ");
        all_text.push_str(&text);

        if let Some(kategori) = tentukan_jenis_dokumen(&text) {
            return Some(kategori);
        }
    }

    tentukan_jenis_dokumen(&all_text)
}

/// Mengekstrak sejumlah halaman tertentu dari PDF asal ke PDF tujuan baru secara lossless
fn extract_pages(source_pdf: &Path, target_pdf: &Path, pages_to_keep: &[u32]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut doc = Document::load(source_pdf)?;
    let all_pages: Vec<u32> = doc.get_pages().keys().cloned().collect();
    let keep_set: HashSet<u32> = pages_to_keep.iter().cloned().collect();
    let to_delete: Vec<u32> = all_pages.into_iter().filter(|p| !keep_set.contains(p)).collect();

    doc.delete_pages(&to_delete);
    doc.prune_objects();
    doc.save(target_pdf)?;
    Ok(())
}

/// Hasil analisis isi berkas PDF
#[derive(Debug)]
enum HasilAnalisisPdf {
    TidakDikenali,
    Tunggal(&'static str),
    Bundel(Vec<(&'static str, Vec<u32>)>), // Dokumen dan daftar halaman 1-based
}

/// Menganalisis berkas PDF: apakah berkas tunggal (1 jenis dokumen) atau bundel (memuat berbagai jenis dokumen)
fn analisis_pdf(path: &Path, engine: &OcrEngine) -> HasilAnalisisPdf {
    let Some(path_str) = path.to_str() else { return HasilAnalisisPdf::TidakDikenali; };
    let h_path = HSTRING::from(path_str);
    let Ok(file_op) = StorageFile::GetFileFromPathAsync(&h_path) else { return HasilAnalisisPdf::TidakDikenali; };
    let Ok(file) = file_op.get() else { return HasilAnalisisPdf::TidakDikenali; };
    let Ok(pdf_doc_op) = PdfDocument::LoadFromFileAsync(&file) else { return HasilAnalisisPdf::TidakDikenali; };
    let Ok(pdf_doc) = pdf_doc_op.get() else { return HasilAnalisisPdf::TidakDikenali; };

    let total_pages = pdf_doc.PageCount().unwrap_or(0);
    if total_pages == 0 {
        return HasilAnalisisPdf::TidakDikenali;
    }

    // Jika hanya 1 halaman
    if total_pages == 1 {
        if let Ok(page) = pdf_doc.GetPage(0) {
            if let Some(kat) = deteksi_halaman_pdf(&page, engine) {
                return HasilAnalisisPdf::Tunggal(kat);
            }
        }
        return HasilAnalisisPdf::TidakDikenali;
    }

    // Jika halaman pertama adalah AJB: AJB adalah satu akta utuh yang tidak dipisah-pisah
    if let Ok(page0) = pdf_doc.GetPage(0) {
        if let Some("AJB") = deteksi_halaman_pdf(&page0, engine) {
            return HasilAnalisisPdf::Tunggal("AJB");
        }
    }

    // Periksa setiap halaman
    let mut page_results: Vec<(u32, Option<&'static str>)> = Vec::new();
    let mut distinct_categories: HashSet<&'static str> = HashSet::new();

    for p_idx in 0..total_pages {
        let page_num = p_idx + 1;
        let kat_opt = if let Ok(page) = pdf_doc.GetPage(p_idx) {
            deteksi_halaman_pdf(&page, engine)
        } else {
            None
        };

        if let Some(k) = kat_opt {
            distinct_categories.insert(k);
        }
        page_results.push((page_num, kat_opt));
    }

    if distinct_categories.is_empty() {
        return HasilAnalisisPdf::TidakDikenali;
    }

    // Jika hanya ada 1 jenis dokumen di seluruh halaman (misal: Sertifikat 2 halaman atau PBB 2 halaman)
    if distinct_categories.len() == 1 {
        let jenis = distinct_categories.into_iter().next().unwrap();
        return HasilAnalisisPdf::Tunggal(jenis);
    }

    // Jika memuat lebih dari 1 jenis dokumen -> BUNDEL MULTI-DOKUMEN
    let mut groups: Vec<(&'static str, Vec<u32>)> = Vec::new();
    for (page_num, kat_opt) in page_results {
        if let Some(kat) = kat_opt {
            if let Some(last_group) = groups.last_mut() {
                if last_group.0 == kat {
                    last_group.1.push(page_num);
                    continue;
                }
            }
            groups.push((kat, vec![page_num]));
        } else {
            // Halaman tidak memiliki kata kunci baru (lampiran/bukti transaksi/lembar lanjutan)
            // Gabungkan ke dokumen sebelumnya yang sedang aktif
            if let Some(last_group) = groups.last_mut() {
                last_group.1.push(page_num);
            }
        }
    }

    if groups.len() <= 1 {
        if let Some((jenis, _)) = groups.first() {
            return HasilAnalisisPdf::Tunggal(jenis);
        }
        return HasilAnalisisPdf::TidakDikenali;
    }

    HasilAnalisisPdf::Bundel(groups)
}

/// Mencatat log ke konsol dan ke file auto_rename.log
fn catat_log(pesan: &str) {
    println!("{}", pesan);
    let log_path = Path::new(r"C:\Users\ryokh\Data\Coding\auto-rename-notary-file\auto_rename.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_path) {
        use std::io::Write;
        let _ = writeln!(f, "{}", pesan);
    }
}

/// Memisahkan berkas bundel menjadi berkas individual di folder yang sama
fn pisahkan_bundel_pdf(path: &Path, groups: &[(&'static str, Vec<u32>)]) -> bool {
    let Some(parent_dir) = path.parent() else { return false; };
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();

    catat_log(&format!(
        " -> [BERKAS BUNDEL TERDETEKSI] Berkas '{}' memuat {} dokumen berbeda. Memulai pemisahan berkas...",
        file_name, groups.len()
    ));

    let mut semua_sukses = true;
    let mut target_tercipta: Vec<PathBuf> = Vec::new();

    for (jenis, pages) in groups {
        let mut target_name = format!("{}.pdf", jenis);
        let mut target_path = parent_dir.join(&target_name);
        let mut counter = 2;
        while (target_path.exists() && target_path != path) || target_tercipta.contains(&target_path) {
            target_name = format!("{}_{}.pdf", jenis, counter);
            target_path = parent_dir.join(&target_name);
            counter += 1;
        }

        let temp_name = format!("_temp_split_{}_{}_{}.pdf", jenis, std::process::id(), pages[0]);
        let temp_path = parent_dir.join(&temp_name);

        if let Err(e) = extract_pages(path, &temp_path, pages) {
            catat_log(&format!("    [Gagal Ekstrak] Dokumen {} halaman {:?}: {}", jenis, pages, e));
            semua_sukses = false;
            let _ = fs::remove_file(&temp_path);
            break;
        }

        if let Err(e) = fs::rename(&temp_path, &target_path) {
            catat_log(&format!("    [Gagal Simpan] {} -> {}: {}", temp_name, target_name, e));
            let _ = fs::remove_file(&temp_path);
            semua_sukses = false;
            break;
        }

        catat_log(&format!(
            "    * Berhasil dipisahkan: '{}' (halaman {:?}) -> {:?}",
            jenis, pages, target_path.file_name().unwrap()
        ));
        target_tercipta.push(target_path);
    }

    if semua_sukses {
        if let Err(e) = fs::remove_file(path) {
            catat_log(&format!("    [Peringatan] Gagal menghapus berkas bundel asal '{}': {}", file_name, e));
        } else {
            catat_log(&format!(" -> SUKSES! Seluruh {} dokumen berhasil dipisahkan dan berkas bundel '{}' telah dibersihkan.", groups.len(), file_name));
        }
        true
    } else {
        catat_log(" -> Pemisahan berkas bundel belum lengkap. Berkas asal tetap dipertahankan.");
        false
    }
}

async fn proses_satu_berkas(path: &Path, engine: &OcrEngine) {
    if !path.exists() {
        return;
    }

    if is_folder_dikecualikan(path) {
        return;
    }

    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else { return };
    let Some(parent_dir) = path.parent() else { return };

    // Jangan proses file yang berada tepat di ROOT_FOLDER (hanya proses yang ada di dalam sub-folder klien)
    if parent_dir == Path::new(ROOT_FOLDER) {
        return;
    }

    let nama_klien = parent_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Klien");

    catat_log(&format!("\n[Mendeteksi Berkas Baru] Folder: {} | File: {}", nama_klien, file_name));

    if !tunggu_file_siap(path).await {
        catat_log(" -> Dilewati: File masih dikunci aplikasi lain.");
        return;
    }

    let path_clone = path.to_path_buf();
    let engine_clone = engine.clone();

    // Jalankan OCR dan analisis di thread pool blocking
    let hasil = tokio::task::spawn_blocking(move || {
        analisis_pdf(&path_clone, &engine_clone)
    })
    .await
    .unwrap_or(HasilAnalisisPdf::TidakDikenali);

    match hasil {
        HasilAnalisisPdf::TidakDikenali => {
            catat_log(&format!(" -> Kata kunci tidak dikenali, nama file dibiarkan tetap: {}", file_name));
        }
        HasilAnalisisPdf::Tunggal(nama_baru) => {
            let mut target_path = parent_dir.join(format!("{}.pdf", nama_baru));
            let mut counter = 2;

            // Jika di dalam folder yang sama ada lebih dari 1 file sejenis (misal 2 KTP), berikutnya dinamai KTP_2.pdf
            while target_path.exists() && target_path != path {
                target_path = parent_dir.join(format!("{}_{}.pdf", nama_baru, counter));
                counter += 1;
            }

            if target_path != path {
                match fs::rename(path, &target_path) {
                    Ok(_) => catat_log(&format!(" -> SUKSES diubah menjadi: {:?}", target_path.file_name().unwrap())),
                    Err(e) => catat_log(&format!(" -> Gagal mengubah nama file: {}", e)),
                }
            }
        }
        HasilAnalisisPdf::Bundel(groups) => {
            let path_clone = path.to_path_buf();
            tokio::task::spawn_blocking(move || {
                pisahkan_bundel_pdf(&path_clone, &groups);
            })
            .await
            .unwrap_or(());
        }
    }
}

/// Mengaudit dan memperbaiki nama file dalam satu folder secara aman dengan pemisahan bundel & teknik 2-fase rename
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

    // Langkah 1: Deteksi dan pisahkan berkas bundel jika ada
    for file_path in files_pdf {
        let file_name = file_path.file_name().unwrap().to_string_lossy().to_string();
        let lower = file_name.to_lowercase();
        let sudah_benar = NAMA_SUDAH_BENAR.iter().any(|&b| {
            lower == b || lower.starts_with(&format!("{}_", b.trim_end_matches(".pdf")))
        });

        // Jika nama file belum standar, periksa apakah bundel
        if !sudah_benar {
            match analisis_pdf(&file_path, engine) {
                HasilAnalisisPdf::Bundel(groups) => {
                    if pisahkan_bundel_pdf(&file_path, &groups) {
                        diperbaiki += groups.len();
                    }
                }
                _ => {}
            }
        }
    }

    // Refresh daftar berkas di folder setelah pemisahan bundel
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

    // Langkah 2: Audit berkas tunggal yang belum tepat namanya
    let mut hasil_deteksi: Vec<(PathBuf, String, &'static str)> = Vec::new();
    for file_path in &files_to_audit {
        let file_name = file_path.file_name().unwrap().to_string_lossy().to_string();
        match analisis_pdf(file_path, engine) {
            HasilAnalisisPdf::Tunggal(jenis) => {
                hasil_deteksi.push((file_path.clone(), file_name, jenis));
            }
            HasilAnalisisPdf::Bundel(_) => {}
            HasilAnalisisPdf::TidakDikenali => {
                catat_log(&format!("  [{}] Berkas '{}': Kata kunci tidak dikenali (dibiarkan)", folder_name, file_name));
            }
        }
    }

    let mut jenis_counters: HashMap<&'static str, usize> = HashMap::new();
    let mut rencana_ubah: Vec<(PathBuf, String, PathBuf)> = Vec::new();

    // Prioritaskan file yang namanya sudah Jenis.pdf agar tetap menjadi Jenis.pdf
    hasil_deteksi.sort_by(|a, b| {
        let a_is_exact = a.1.to_lowercase() == format!("{}.pdf", a.2.to_lowercase());
        let b_is_exact = b.1.to_lowercase() == format!("{}.pdf", b.2.to_lowercase());
        b_is_exact.cmp(&a_is_exact)
    });

    for (current_path, current_name, jenis) in &hasil_deteksi {
        let count = jenis_counters.entry(jenis).or_insert(0);
        *count += 1;

        let target_file_name = if *count == 1 {
            format!("{}.pdf", jenis)
        } else {
            format!("{}_{}.pdf", jenis, count)
        };

        let target_path = folder.join(&target_file_name);

        if current_name.to_lowercase() != target_file_name.to_lowercase() {
            rencana_ubah.push((current_path.clone(), current_name.clone(), target_path));
        }
    }

    if rencana_ubah.is_empty() {
        return diperbaiki;
    }

    catat_log(&format!("  [Audit Folder] '{}': Ditemukan {} berkas yang salah nama. Memperbaiki...", folder_name, rencana_ubah.len()));

    // Fase 1: Pindahkan ke nama sementara untuk menghindari benturan nama
    let mut temp_items: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (i, (current_path, current_name, final_target)) in rencana_ubah.into_iter().enumerate() {
        let temp_name = format!("_temp_audit_{}_{}.pdf", i, current_path.file_stem().unwrap().to_string_lossy());
        let temp_path = folder.join(&temp_name);
        if let Err(e) = fs::rename(&current_path, &temp_path) {
            catat_log(&format!("    [Gagal Temp] {} -> {}: {}", current_name, temp_name, e));
            continue;
        }
        temp_items.push((temp_path, final_target));
    }

    // Fase 2: Pindahkan dari nama sementara ke nama target final
    for (temp_path, mut final_target) in temp_items {
        let mut target_name = final_target.file_name().unwrap().to_string_lossy().to_string();
        let stem = final_target.file_stem().unwrap().to_string_lossy().to_string();
        let mut counter = 2;
        while final_target.exists() && final_target != temp_path {
            let alt_name = format!("{}_{}.pdf", stem, counter);
            final_target = folder.join(&alt_name);
            target_name = alt_name;
            counter += 1;
        }

        match fs::rename(&temp_path, &final_target) {
            Ok(_) => {
                catat_log(&format!("    * Berhasil diubah -> {}", target_name));
                diperbaiki += 1;
            }
            Err(e) => catat_log(&format!("    * Gagal mengubah ke {}: {}", target_name, e)),
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

    // Jika ini bukan ROOT_FOLDER, lakukan audit pada berkas di dalam folder ini
    if dir != Path::new(ROOT_FOLDER) {
        *total_folder += 1;
        let perbaikan = audit_dan_perbaiki_satu_folder(dir, engine);
        *total_diperbaiki += perbaikan;
    }

    // Masuk ke sub-folder aktif jika ada
    for sub in subdirs {
        audit_folder_rekursif(&sub, engine, total_folder, total_diperbaiki);
    }
}

/// Menjalankan audit menyeluruh ke seluruh folder klien aktif secara rekursif saat program baru dinyalakan
fn audit_semua_folder_aktif(engine: &OcrEngine) {
    catat_log("------------------------------------------------------");
    catat_log("[AUDIT STARTUP] Memulai pengecekan seluruh folder klien...");
    catat_log("Tiap berkas diperiksa ulang isinya menggunakan OCR.");
    catat_log("Jika ada berkas bundel multi-dokumen, akan dipisahkan otomatis.");
    catat_log("Jika ada nama file yang masih salah, akan diperbaiki otomatis.");
    catat_log("------------------------------------------------------");

    let root = Path::new(ROOT_FOLDER);
    let mut total_folder = 0;
    let mut total_diperbaiki = 0;

    audit_folder_rekursif(root, engine, &mut total_folder, &mut total_diperbaiki);

    catat_log("------------------------------------------------------");
    catat_log(&format!(
        "[AUDIT SELESAI] Total {} folder klien diperiksa. Total {} tindakan perbaikan.",
        total_folder, total_diperbaiki
    ));
    catat_log("Seluruh folder klien aktif sudah terverifikasi dan rapi.");
    catat_log("------------------------------------------------------");
}

#[tokio::main]
async fn main() -> notify::Result<()> {
    catat_log("======================================================");
    catat_log(" AUTO-RENAME NOTARIS AKTIF (NATIVE WINRT PDF & OCR) ");
    catat_log(&format!(" Memantau Folder : {}", ROOT_FOLDER));
    catat_log(" Fitur           : Pemisahan Bundel Multi-Dokumen & Auto-Rename");
    catat_log(" Pengecualian    : Folder tidak kapital semua dilewati (finish, revisi, old data, dll)");
    catat_log("======================================================");

    // Menunggu Google Drive terhubung dan folder target tersedia (terutama saat baru login/boot)
    let mut percobaan = 0;
    while !Path::new(ROOT_FOLDER).exists() {
        if percobaan == 0 {
            catat_log("Menunggu Google Drive (G:\\) terhubung dan siap...");
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
        percobaan += 1;
        if percobaan >= 60 {
            catat_log("Peringatan: Folder belum ditemukan setelah 3 menit. Tetap menunggu Google Drive...");
            percobaan = 0;
        }
    }
    catat_log("Folder target terhubung! Menginisialisasi OCR...");

    let ocr_engine = match OcrEngine::TryCreateFromUserProfileLanguages() {
        Ok(engine) => engine,
        Err(e) => {
            catat_log(&format!("Gagal menginisialisasi Windows OCR Engine: {}", e));
            return Ok(());
        }
    };

    // 1. Jalankan audit menyeluruh saat startup untuk mengecek dan memperbaiki semua folder aktif
    let ocr_engine_audit = ocr_engine.clone();
    tokio::task::spawn_blocking(move || {
        audit_semua_folder_aktif(&ocr_engine_audit);
    })
    .await
    .unwrap_or(());

    // 2. Siapkan antrean terpisah untuk berkas baru dan folder yang baru di-rename menjadi kapital
    let (tx_file, mut rx_file) = mpsc::channel::<PathBuf>(1000);
    let (tx_folder, mut rx_folder) = mpsc::channel::<PathBuf>(100);

    let sedang_diproses_file: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
    let sedang_diproses_file_watcher = Arc::clone(&sedang_diproses_file);

    let sedang_diproses_folder: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
    let sedang_diproses_folder_watcher = Arc::clone(&sedang_diproses_folder);

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                    for path in event.paths {
                        if is_folder_dikecualikan(&path) {
                            continue;
                        }

                        // KASUS A: Event pada FOLDER (misal: folder baru dibuat atau DI-RENAME menjadi huruf kapital)
                        let is_dir = path.is_dir()
                            || (!path.to_string_lossy().to_lowercase().ends_with(".pdf") && path.exists());

                        if is_dir && path != Path::new(ROOT_FOLDER) {
                            let mut set = sedang_diproses_folder_watcher.lock().unwrap();
                            if !set.contains(&path) {
                                set.insert(path.clone());
                                let _ = tx_folder.blocking_send(path.clone());
                            }
                            continue;
                        }

                        // KASUS B: Event pada BERKAS PDF
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            let lower = name.to_lowercase();
                            // Abaikan semua berkas temporary (_temp_audit_, _temp_split_, dll.)
                            if lower.starts_with("_temp_") {
                                continue;
                            }
                            // Proses semua file .pdf yang namanya BELUM termasuk dalam 10 nama standar
                            let sudah_benar = NAMA_SUDAH_BENAR.iter().any(|&b| {
                                lower == b || lower.starts_with(&format!("{}_", b.trim_end_matches(".pdf")))
                            });

                            if lower.ends_with(".pdf") && !sudah_benar {
                                let mut set = sedang_diproses_file_watcher.lock().unwrap();
                                if !set.contains(&path) {
                                    set.insert(path.clone());
                                    let _ = tx_file.blocking_send(path);
                                }
                            }
                        }
                    }
                }
            }
        },
        Config::default(),
    )?;

    watcher.watch(Path::new(ROOT_FOLDER), RecursiveMode::Recursive)?;

    // Worker untuk menangani folder yang baru di-rename menjadi kapital
    let ocr_engine_folder_worker = ocr_engine.clone();
    let sedang_diproses_folder_cleanup = Arc::clone(&sedang_diproses_folder);
    tokio::spawn(async move {
        while let Some(folder_path) = rx_folder.recv().await {
            // Beri jeda 800ms agar proses rename Windows Explorer / Google Drive selesai sempurna
            tokio::time::sleep(Duration::from_millis(800)).await;

            if !folder_path.exists() || is_folder_dikecualikan(&folder_path) {
                sedang_diproses_folder_cleanup.lock().unwrap().remove(&folder_path);
                continue;
            }

            let folder_name = folder_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Folder".to_string());

            catat_log(&format!(
                "\n[Folder Aktif Terdeteksi / Di-rename Menjadi Kapital] Memeriksa berkas di folder: '{}'",
                folder_name
            ));

            let ocr = ocr_engine_folder_worker.clone();
            let fp = folder_path.clone();

            tokio::task::spawn_blocking(move || {
                let mut total_f = 0;
                let mut total_d = 0;
                audit_folder_rekursif(&fp, &ocr, &mut total_f, &mut total_d);
                if total_d > 0 {
                    catat_log(&format!("  -> Selesai! Sebanyak {} berkas di '{}' telah diperbaiki.", total_d, folder_name));
                } else {
                    catat_log(&format!("  -> Pemeriksaan selesai. Seluruh berkas di '{}' sudah tepat.", folder_name));
                }
            })
            .await
            .unwrap_or(());

            sedang_diproses_folder_cleanup.lock().unwrap().remove(&folder_path);
        }
    });

    catat_log("\n[Pemantau Realtime Aktif] Siap memproses berkas baru & memisahkan bundel PDF secara otomatis...");

    // Worker untuk menangani berkas PDF baru secara realtime
    while let Some(path) = rx_file.recv().await {
        proses_satu_berkas(&path, &ocr_engine).await;
        sedang_diproses_file.lock().unwrap().remove(&path);
    }

    Ok(())
}