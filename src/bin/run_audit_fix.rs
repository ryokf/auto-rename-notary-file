use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use windows::{
    core::HSTRING,
    Data::Pdf::{PdfDocument, PdfPageRenderOptions},
    Graphics::Imaging::{
        BitmapDecoder, BitmapRotation, BitmapTransform, ColorManagementMode, ExifOrientationMode,
    },
    Media::Ocr::OcrEngine,
    Storage::StorageFile,
    Storage::Streams::InMemoryRandomAccessStream,
};

const ROOT_FOLDER: &str = r"G:\My Drive\notaris bu sri\Pihak Kedua";

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

    // 2. Permohonan.pdf (Formulir permohonan ke Kepala Kantor Pertanahan)
    if (t.contains("KEPALA KANTOR PERTANAHAN") || t.contains("KANTOR PERTANAHAN"))
        && (t.contains("MENGAJUKAN PERMOHONAN")
            || t.contains("PENDAFTARAN PERALIHAN HAK")
            || t.contains("UNTUK MELENGKAPI PERMOHONAN")
            || t.contains("SURAT PERMOHONAN"))
    {
        return Some("Permohonan");
    }

    // 3. Cek.pdf (Hasil Pengecekan Sertipikat)
    if t.contains("PENGECEKAN SERTIPIKAT")
        || t.contains("DAFTAR HAK TANGGUNGAN")
        || t.contains("TIDAK SEDANG DIAGUNKAN")
        || t.contains("TIDAK TERDAPAT BLOKIR")
        || t.contains("HASIL PENGECEKAN")
    {
        return Some("Cek");
    }

    // 4. PPH.pdf
    if t.contains("PENELITIAN BUKTI PEMENUHAN")
        || t.contains("PAJAK PENGHASILAN")
        || t.contains("411128")
        || t.contains("PHTB-CT")
        || (t.contains("SURAT SETORAN PAJAK") && t.contains("PPH"))
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

    // 7. KK.pdf (Kartu Keluarga - dicek sebelum Sertifikat karena KK modern memuat catatan kaki 'sertifikat elektronik BSrE')
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

    // 8. AJB.pdf (Akta Jual Beli otentik PPAT - bukan sertipikat tanah yang memuat kutipan akta jual beli)
    if !t.contains("KEMENTERIAN AGRARIA") && !t.contains("BADAN PERTANAHAN") && !t.contains("EDISI") {
        if t.contains("AKTA JUAL BELI")
            || t.contains("AKTA GUAL BELI")
            || (t.contains("AKTA") && t.contains("BELI") && (t.contains("PPAT") || t.contains("PEJABAT PEMBUAT AKTA TANAH")))
        {
            return Some("AJB");
        }
    }

    // 9. Sertifikat.pdf (Sertipikat Hak Tanah BPN, baik buku fisik hijau maupun sertipikat elektronik putih)
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

    // 10. KTP.pdf (KTP biasa, e-KTP, atau fotokopi KTP yang dilegalisir notaris)
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
            && (t.contains("DEMAK") || t.contains("WAHYUNINGSIH") || t.contains("LAHIR") || t.contains("KABUPATEN") || t.contains("ASLI")));

    if is_ktp {
        return Some("KTP");
    }

    None
}

fn deteksi_jenis_dokumen_pdf(path: &Path, engine: &OcrEngine) -> Option<&'static str> {
    let path_str = path.to_str()?;
    let h_path = HSTRING::from(path_str);
    let file = StorageFile::GetFileFromPathAsync(&h_path).ok()?.get().ok()?;
    let pdf_doc = PdfDocument::LoadFromFileAsync(&file).ok()?.get().ok()?;

    let total_pages = pdf_doc.PageCount().unwrap_or(0);
    if total_pages == 0 {
        return None;
    }

    let max_check = total_pages.min(2);

    for p_idx in 0..max_check {
        let Ok(page) = pdf_doc.GetPage(p_idx) else { continue };
        let Ok(size) = page.Size() else { continue };

        let scale: f32 = if size.Width > 1000.0 || size.Height > 1000.0 { 1.5 } else { 2.2 };
        let Ok(render_options) = PdfPageRenderOptions::new() else { continue };
        let _ = render_options.SetDestinationWidth((size.Width * scale) as u32);
        let _ = render_options.SetDestinationHeight((size.Height * scale) as u32);

        let Ok(stream) = InMemoryRandomAccessStream::new() else { continue };
        let Ok(render_op) = page.RenderWithOptionsToStreamAsync(&stream, &render_options) else { continue };
        if render_op.get().is_err() {
            continue;
        }

        let Ok(decoder_op) = BitmapDecoder::CreateAsync(&stream) else { continue };
        let Ok(decoder) = decoder_op.get() else { continue };

        for rotation in [
            BitmapRotation::None,
            BitmapRotation::Clockwise270Degrees,
            BitmapRotation::Clockwise90Degrees,
            BitmapRotation::Clockwise180Degrees,
        ] {
            let Ok(transform) = BitmapTransform::new() else { continue };
            let _ = transform.SetRotation(rotation);

            let Ok(pixel_format) = decoder.BitmapPixelFormat() else { continue };
            let Ok(alpha_mode) = decoder.BitmapAlphaMode() else { continue };

            let Ok(transform_op) = decoder.GetSoftwareBitmapTransformedAsync(
                pixel_format,
                alpha_mode,
                &transform,
                ExifOrientationMode::IgnoreExifOrientation,
                ColorManagementMode::DoNotColorManage,
            ) else { continue };

            let Ok(software_bitmap) = transform_op.get() else { continue };

            let Ok(ocr_op) = engine.RecognizeAsync(&software_bitmap) else { continue };
            let Ok(ocr_result) = ocr_op.get() else { continue };
            let Ok(text) = ocr_result.Text() else { continue };

            let text_str = text.to_string();
            if let Some(kategori) = tentukan_jenis_dokumen(&text_str) {
                return Some(kategori);
            }
        }
    }

    None
}

fn perbaiki_folder(folder: &Path, engine: &OcrEngine) {
    let folder_name = folder.file_name().unwrap_or_default().to_string_lossy();
    let Ok(entries) = fs::read_dir(folder) else { return };

    let mut files_pdf = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()) == Some("pdf".to_string()) {
            files_pdf.push(path);
        }
    }

    // Kelompokkan file berdasarkan hasil OCR
    // jenis -> list file
    let mut hasil_deteksi: Vec<(PathBuf, String, &'static str)> = Vec::new();
    for file_path in files_pdf {
        let file_name = file_path.file_name().unwrap().to_string_lossy().to_string();
        if let Some(jenis) = deteksi_jenis_dokumen_pdf(&file_path, engine) {
            hasil_deteksi.push((file_path, file_name, jenis));
        } else {
            println!("  [LEWATI] {} (Tidak dikenali, dibiarkan)", file_name);
        }
    }

    // Tentukan nama target ideal untuk setiap file
    // Untuk jenis yang sama: file pertama = Jenis.pdf, kedua = Jenis_2.pdf, dst.
    let mut jenis_counters: HashMap<&'static str, usize> = HashMap::new();
    let mut rencana_ubah: Vec<(PathBuf, String, PathBuf)> = Vec::new();

    // Urutkan agar file yang namanya sudah mendekati Jenis.pdf diprioritaskan mendapat Jenis.pdf
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

        // Jika nama saat ini berbeda dengan target_path (case-insensitive perbandingan nama)
        if current_name.to_lowercase() != target_file_name.to_lowercase() {
            rencana_ubah.push((current_path.clone(), current_name.clone(), target_path));
        }
    }

    if rencana_ubah.is_empty() {
        println!(">>> Folder {}: SEMUA FILE SUDAH TEPAT. Tidak ada perubahan.", folder_name);
        return;
    }

    println!("\n>>> Folder {}: DITEMUKAN {} FILE YANG SALAH NAMA. Memperbaiki...", folder_name, rencana_ubah.len());

    // Fase 1: Ubah ke nama sementara (temp) untuk mencegah tabrakan/overwrite
    let mut temp_items: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (i, (current_path, current_name, final_target)) in rencana_ubah.into_iter().enumerate() {
        let temp_name = format!("_temp_audit_{}_{}.pdf", i, current_path.file_stem().unwrap().to_string_lossy());
        let temp_path = folder.join(&temp_name);
        if let Err(e) = fs::rename(&current_path, &temp_path) {
            eprintln!("  Gagal memindahkan {} ke {}: {}", current_name, temp_name, e);
            continue;
        }
        temp_items.push((temp_path, final_target));
    }

    // Fase 2: Ubah dari nama sementara ke nama target final
    for (temp_path, final_target) in temp_items {
        let target_name = final_target.file_name().unwrap().to_string_lossy().to_string();
        match fs::rename(&temp_path, &final_target) {
            Ok(_) => println!("  [BERHASIL DIUBAH] -> {}", target_name),
            Err(e) => eprintln!("  [GAGAL] {} -> {}: {}", temp_path.file_name().unwrap().to_string_lossy(), target_name, e),
        }
    }
}

fn main() {
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().unwrap();
    let root = Path::new(ROOT_FOLDER);

    println!("============================================================");
    println!("EKSEKUSI PERBAIKAN SEMUA FOLDER KLIEN AKTIF");
    println!("============================================================");

    let entries = fs::read_dir(root).unwrap();
    for entry in entries.flatten() {
        let folder = entry.path();
        if !folder.is_dir() || is_folder_dikecualikan(&folder) {
            continue;
        }
        perbaiki_folder(&folder, &engine);
    }

    println!("\n============================================================");
    println!("AUDIT DAN PERBAIKAN SELESAI!");
    println!("============================================================");
}
