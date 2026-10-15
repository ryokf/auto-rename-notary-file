use lopdf::Document;
use std::collections::{HashMap, HashSet};
use std::fs;
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

const ROOT_FOLDER: &str = r"G:\My Drive\notaris bu sri\Pihak Kedua";

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

    // 2. Permohonan.pdf
    if (t.contains("KEPALA KANTOR PERTANAHAN") || t.contains("KANTOR PERTANAHAN"))
        && (t.contains("MENGAJUKAN PERMOHONAN")
            || t.contains("PENDAFTARAN PERALIHAN HAK")
            || t.contains("UNTUK MELENGKAPI PERMOHONAN")
            || t.contains("SURAT PERMOHONAN"))
    {
        return Some("Permohonan");
    }

    // 3. Cek.pdf
    if t.contains("PENGECEKAN SERTIPIKAT")
        || t.contains("DAFTAR HAK TANGGUNGAN")
        || t.contains("TIDAK SEDANG DIAGUNKAN")
        || t.contains("TIDAK TERDAPAT BLOKIR")
        || t.contains("HASIL PENGECEKAN")
    {
        return Some("Cek");
    }

    // 4. PPH.pdf (Surat penelitian, billing DJP, dan bukti bayar/NTPN)
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

    // 5. PBB.pdf
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

    // 6. BPHTB.pdf
    if t.contains("SSPD")
        || t.contains("SURAT SETORAN PAJAK DAERAH")
        || t.contains("BEA PEROLEHAN HAK")
        || t.contains("NPOPTKP")
        || t.contains("BPHTB")
        || (t.contains("PAJAK DAERAH") && (t.contains("BEA") || t.contains("HAK") || t.contains("SETORAN") || t.contains("SETU-AN")))
    {
        return Some("BPHTB");
    }

    // 7. KK.pdf
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

    // 8. AJB.pdf
    if !t.contains("KEMENTERIAN AGRARIA") && !t.contains("BADAN PERTANAHAN") && !t.contains("EDISI") {
        if t.contains("AKTA JUAL BELI")
            || t.contains("AKTA GUAL BELI")
            || (t.contains("AKTA") && t.contains("BELI") && (t.contains("PPAT") || t.contains("PEJABAT PEMBUAT AKTA TANAH")))
        {
            return Some("AJB");
        }
    }

    // 9. Sertifikat.pdf
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

    // 10. KTP.pdf
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

#[derive(Debug)]
enum HasilAnalisisPdf {
    TidakDikenali,
    Tunggal(&'static str),
    Bundel(Vec<(&'static str, Vec<u32>)>),
}

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

    if total_pages == 1 {
        if let Ok(page) = pdf_doc.GetPage(0) {
            if let Some(kat) = deteksi_halaman_pdf(&page, engine) {
                return HasilAnalisisPdf::Tunggal(kat);
            }
        }
        return HasilAnalisisPdf::TidakDikenali;
    }

    // Prioritas Khusus 1: AJB adalah satu akta otentik PPAT utuh yang tidak boleh dipisah-pisah
    if let Ok(page0) = pdf_doc.GetPage(0) {
        if let Some("AJB") = deteksi_halaman_pdf(&page0, engine) {
            return HasilAnalisisPdf::Tunggal("AJB");
        }
    }
    if total_pages > 1 {
        if let Ok(page1) = pdf_doc.GetPage(1) {
            if let Some("AJB") = deteksi_halaman_pdf(&page1, engine) {
                return HasilAnalisisPdf::Tunggal("AJB");
            }
        }
    }

    // Prioritas Khusus 2: Sertifikat adalah buku/sertipikat hak tanah BPN utuh yang tidak boleh dipisah-pisah
    if let Ok(page0) = pdf_doc.GetPage(0) {
        if let Some("Sertifikat") = deteksi_halaman_pdf(&page0, engine) {
            return HasilAnalisisPdf::Tunggal("Sertifikat");
        }
    }
    if total_pages > 1 {
        if let Ok(page1) = pdf_doc.GetPage(1) {
            if let Some("Sertifikat") = deteksi_halaman_pdf(&page1, engine) {
                return HasilAnalisisPdf::Tunggal("Sertifikat");
            }
        }
    }

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

    if distinct_categories.len() == 1 {
        let jenis = distinct_categories.into_iter().next().unwrap();
        return HasilAnalisisPdf::Tunggal(jenis);
    }

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

fn pisahkan_bundel_pdf(path: &Path, groups: &[(&'static str, Vec<u32>)]) -> bool {
    let Some(parent_dir) = path.parent() else { return false; };
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();

    println!(
        " -> [BERKAS BUNDEL TERDETEKSI] Berkas '{}' memuat {} dokumen berbeda. Memulai pemisahan berkas...",
        file_name, groups.len()
    );

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
            println!("    [Gagal Ekstrak] Dokumen {} halaman {:?}: {}", jenis, pages, e);
            semua_sukses = false;
            let _ = fs::remove_file(&temp_path);
            break;
        }

        if let Err(e) = fs::rename(&temp_path, &target_path) {
            println!("    [Gagal Simpan] {} -> {}: {}", temp_name, target_name, e);
            let _ = fs::remove_file(&temp_path);
            semua_sukses = false;
            break;
        }

        println!(
            "    * Berhasil dipisahkan: '{}' (halaman {:?}) -> {:?}",
            jenis, pages, target_path.file_name().unwrap()
        );
        target_tercipta.push(target_path);
    }

    if semua_sukses {
        if let Err(e) = fs::remove_file(path) {
            println!("    [Peringatan] Gagal menghapus berkas bundel asal '{}': {}", file_name, e);
        } else {
            println!(" -> SUKSES! Seluruh {} dokumen berhasil dipisahkan dan berkas bundel '{}' telah dibersihkan.", groups.len(), file_name);
        }
        true
    } else {
        println!(" -> Pemisahan berkas bundel belum lengkap. Berkas asal tetap dipertahankan.");
        false
    }
}

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

    // Langkah 1: Pisahkan berkas bundel jika ada
    for file_path in files_pdf {
        let file_name = file_path.file_name().unwrap().to_string_lossy().to_string();
        let lower = file_name.to_lowercase();
        let sudah_benar = NAMA_SUDAH_BENAR.iter().any(|&b| {
            lower == b || lower.starts_with(&format!("{}_", b.trim_end_matches(".pdf")))
        });

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

    // Refresh daftar berkas di folder
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

    let mut hasil_deteksi: Vec<(PathBuf, String, &'static str)> = Vec::new();
    for file_path in &files_to_audit {
        let file_name = file_path.file_name().unwrap().to_string_lossy().to_string();
        match analisis_pdf(file_path, engine) {
            HasilAnalisisPdf::Tunggal(jenis) => {
                hasil_deteksi.push((file_path.clone(), file_name, jenis));
            }
            HasilAnalisisPdf::Bundel(_) => {}
            HasilAnalisisPdf::TidakDikenali => {
                println!("  [{}] Berkas '{}': Kata kunci tidak dikenali (dibiarkan)", folder_name, file_name);
            }
        }
    }

    let mut jenis_counters: HashMap<&'static str, usize> = HashMap::new();
    let mut rencana_ubah: Vec<(PathBuf, String, PathBuf)> = Vec::new();

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

    println!("  [Audit Folder] '{}': Ditemukan {} berkas yang salah nama. Memperbaiki...", folder_name, rencana_ubah.len());

    let mut temp_items: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (i, (current_path, current_name, final_target)) in rencana_ubah.into_iter().enumerate() {
        let temp_name = format!("_temp_audit_{}_{}.pdf", i, current_path.file_stem().unwrap().to_string_lossy());
        let temp_path = folder.join(&temp_name);
        if let Err(e) = fs::rename(&current_path, &temp_path) {
            println!("    [Gagal Temp] {} -> {}: {}", current_name, temp_name, e);
            continue;
        }
        temp_items.push((temp_path, final_target));
    }

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
                println!("    * Berhasil diubah -> {}", target_name);
                diperbaiki += 1;
            }
            Err(e) => println!("    * Gagal mengubah ke {}: {}", target_name, e),
        }
    }

    diperbaiki
}

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
    println!("======================================================");
    println!(" AUDIT & PERBAIKAN NAMA DOKUMEN NOTARIS + SPLIT BUNDEL");
    println!(" Target Folder : {}", ROOT_FOLDER);
    println!(" Pengecualian  : Folder huruf kecil (finish, revisi, dll)");
    println!("======================================================");

    let engine = match OcrEngine::TryCreateFromUserProfileLanguages() {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("Gagal menginisialisasi Windows OCR Engine: {}", e);
            return;
        }
    };

    let mut total_folder = 0;
    let mut total_diperbaiki = 0;
    let root = Path::new(ROOT_FOLDER);

    audit_folder_rekursif(root, &engine, &mut total_folder, &mut total_diperbaiki);

    println!("======================================================");
    println!("Total {} folder klien diperiksa.", total_folder);
    println!("Total {} berkas diperbaiki / bundel dipisahkan.", total_diperbaiki);
    println!("Semua berkas pada folder klien aktif telah rapi!");
    println!("======================================================");
}
