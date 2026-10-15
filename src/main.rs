use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
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

// ==================== PENGATURAN UTAMA ====================
const ROOT_FOLDER: &str = r"G:\My Drive\notaris bu sri\Pihak Kedua";

// Daftar 10 nama file yang sudah benar (agar tidak diproses ulang oleh pemantau folder)
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
        // Lewati jika ini adalah nama berkas PDF
        if name.to_lowercase().ends_with(".pdf") {
            continue;
        }

        // Jika nama folder memiliki huruf kecil (artinya tidak berhuruf kapital semua)
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

    // 2. Permohonan.pdf (Dicek sebelum Cek & Sertifikat karena formulir Permohonan memuat checklist 'Pengecekan Sertipikat')
    if (t.contains("KEPALA KANTOR PERTANAHAN") || t.contains("KANTOR PERTANAHAN"))
        && (t.contains("MENGAJUKAN PERMOHONAN")
            || t.contains("PENDAFTARAN PERALIHAN HAK")
            || t.contains("UNTUK MELENGKAPI PERMOHONAN"))
    {
        return Some("Permohonan");
    }

    // 3. Cek.pdf (Dicek sebelum Sertifikat karena sama-sama memuat kata Sertipikat)
    if t.contains("PENGECEKAN SERTIPIKAT")
        || t.contains("DAFTAR HAK TANGGUNGAN")
        || t.contains("TIDAK SEDANG DIAGUNKAN")
        || t.contains("TIDAK TERDAPAT BLOKIR")
    {
        return Some("Cek");
    }

    // 4. PPH.pdf
    if t.contains("PENELITIAN BUKTI PEMENUHAN")
        || t.contains("PAJAK PENGHASILAN")
        || t.contains("411128")
        || t.contains("PHTB-CT")
    {
        return Some("PPH");
    }

    // 5. BPHTB.pdf
    if t.contains("SSPD")
        || t.contains("SURAT SETORAN PAJAK DAERAH")
        || t.contains("BEA PEROLEHAN HAK ATAS TANAH")
        || t.contains("NPOPTKP")
        || t.contains("BPHTB")
    {
        return Some("BPHTB");
    }

    // 6. PBB.pdf (Mencakup SPPT PBB maupun Surat Keterangan NJOP)
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

    // 7. KK.pdf
    if t.contains("KARTU KELUARGA")
        || t.contains("NAMA KEPALA KELUARGA")
        || t.contains("KEPALA KELUARGA")
        || ((t.contains("KEPALA") || t.contains("PENCATATAN SIPIL") || t.contains("KEPENDUDUKAN"))
            && (t.contains("RT/RW") || t.contains("ALAMAT"))
            && (t.contains("AYAH") || t.contains("IBU") || t.contains("HUBUNGAN") || t.contains("LAHIR")))
    {
        return Some("KK");
    }

    // 8. KTP.pdf
    let is_ktp = t.contains("KARTU TANDA PENDUDUK")
        || t.contains("SEUMUR")
        || t.contains("SEIJMUR")
        || (t.contains("NIK") && (t.contains("33") || t.contains("PROVINSI") || t.contains("KABUPATEN") || t.contains("KOTA") || t.contains("AGAMA") || t.contains("KAWIN") || t.contains("WARGA")))
        || ((t.contains("PROVINSI") || t.contains("PROV")) && (t.contains("KABUPATEN") || t.contains("PATI") || t.contains("DEMAK") || t.contains("GROBOGAN") || t.contains("SEMARANG")))
        || ((t.contains("JENIS KELAMIN")
            || t.contains("STATUS PERKAWINAN")
            || t.contains("KEWARGANEGARAAN")
            || t.contains("TEMPAT/TGL LAHIR")
            || t.contains("GOL. DARAH"))
            && (t.contains("NIK") || t.contains("PROVINSI") || t.contains("KABUPATEN") || t.contains("INDONESIA")));

    if is_ktp {
        return Some("KTP");
    }

    // 9. AJB.pdf
    if t.contains("AKTA JUAL BELI")
        || t.contains("AKTA GUAL BELI")
        || (t.contains("AKTA") && t.contains("BELI") && (t.contains("PPAT") || t.contains("PEJABAT PEMBUAT AKTA TANAH")))
    {
        return Some("AJB");
    }

    // 10. Sertifikat.pdf
    if t.contains("SERTIPIKAT HAK")
        || t.contains("SERTIFIKAT HAK")
        || (t.contains("PEMEGANG HAK") && t.contains("BIDANG TANAH"))
        || ((t.contains("SERTIPIKAT") || t.contains("SERTIFIKAT"))
            && (t.contains("PERTANAHAN") || t.contains("AGRARIA") || t.contains("DAFTAR ISIAN") || t.contains("SURAT UKUR") || t.contains("BADAN PERTANAHAN")))
    {
        return Some("Sertifikat");
    }

    None
}

/// Merender halaman PDF dan menjalankan Windows Media OCR langsung di memori dengan berbagai rotasi (0°, 270°, 90°, 180°)
fn deteksi_jenis_dokumen_pdf(path: &Path, engine: &OcrEngine) -> Option<&'static str> {
    let path_str = path.to_str()?;
    let h_path = HSTRING::from(path_str);
    let file = StorageFile::GetFileFromPathAsync(&h_path).ok()?.get().ok()?;
    let pdf_doc = PdfDocument::LoadFromFileAsync(&file).ok()?.get().ok()?;

    let total_pages = pdf_doc.PageCount().unwrap_or(0);
    if total_pages == 0 {
        return None;
    }

    // Cek halaman pertama (indeks 0). Jika belum terdeteksi dan ada halaman berikutnya, cek halaman kedua (indeks 1)
    let max_check = total_pages.min(2);

    for p_idx in 0..max_check {
        let Ok(page) = pdf_doc.GetPage(p_idx) else { continue };
        let Ok(size) = page.Size() else { continue };

        let Ok(render_options) = PdfPageRenderOptions::new() else { continue };
        // Naikkan resolusi ke 2.5x agar pembacaan OCR tajam dan akurat
        let _ = render_options.SetDestinationWidth((size.Width * 2.5) as u32);
        let _ = render_options.SetDestinationHeight((size.Height * 2.5) as u32);

        let Ok(stream) = InMemoryRandomAccessStream::new() else { continue };
        let Ok(render_op) = page.RenderWithOptionsToStreamAsync(&stream, &render_options) else { continue };
        if render_op.get().is_err() {
            continue;
        }

        let Ok(decoder_op) = BitmapDecoder::CreateAsync(&stream) else { continue };
        let Ok(decoder) = decoder_op.get() else { continue };

        // Prioritas rotasi: 0° normal, 270° (landscape khas scanner), 90°, 180°
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

    println!("\n[Mendeteksi Berkas] Folder: {} | File: {}", nama_klien, file_name);

    if !tunggu_file_siap(path).await {
        println!(" -> Dilewati: File masih dikunci aplikasi lain.");
        return;
    }

    let path_clone = path.to_path_buf();
    let engine_clone = engine.clone();

    // Jalankan OCR di thread pool blocking
    let hasil_kategori = tokio::task::spawn_blocking(move || {
        deteksi_jenis_dokumen_pdf(&path_clone, &engine_clone)
    })
    .await
    .unwrap_or(None);

    if let Some(nama_baru) = hasil_kategori {
        let mut target_path = parent_dir.join(format!("{}.pdf", nama_baru));
        let mut counter = 2;

        // Jika di dalam folder yang sama ada lebih dari 1 file sejenis (misal 2 KTP), berikutnya dinamai KTP_2.pdf
        while target_path.exists() && target_path != path {
            target_path = parent_dir.join(format!("{}_{}.pdf", nama_baru, counter));
            counter += 1;
        }

        match fs::rename(path, &target_path) {
            Ok(_) => println!(" -> SUKSES diubah menjadi: {:?}", target_path.file_name().unwrap()),
            Err(e) => eprintln!(" -> Gagal mengubah nama file: {}", e),
        }
    } else {
        println!(" -> Kata kunci tidak dikenali, nama file dibiarkan tetap: {}", file_name);
    }
}

/// Memindai seluruh folder klien berhuruf kapital saat aplikasi baru pertama kali dijalankan
async fn pindai_folder_awal(
    dir: &Path,
    tx: &mpsc::Sender<PathBuf>,
    sedang_diproses: &Arc<Mutex<HashSet<PathBuf>>>,
) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !is_folder_dikecualikan(&path) {
                Box::pin(pindai_folder_awal(&path, tx, sedang_diproses)).await;
            }
        } else if path.is_file() {
            if is_folder_dikecualikan(&path) {
                continue;
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let lower = name.to_lowercase();
                let sudah_benar = NAMA_SUDAH_BENAR.iter().any(|&b| {
                    lower == b || lower.starts_with(&format!("{}_", b.trim_end_matches(".pdf")))
                });

                if lower.ends_with(".pdf") && !sudah_benar {
                    let sudah_ada = {
                        let mut set = sedang_diproses.lock().unwrap();
                        if !set.contains(&path) {
                            set.insert(path.clone());
                            false
                        } else {
                            true
                        }
                    };

                    if !sudah_ada {
                        let _ = tx.send(path).await;
                    }
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> notify::Result<()> {
    if !Path::new(ROOT_FOLDER).exists() {
        let _ = fs::create_dir_all(ROOT_FOLDER);
    }

    let ocr_engine = match OcrEngine::TryCreateFromUserProfileLanguages() {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("Gagal menginisialisasi Windows OCR Engine: {}", e);
            return Ok(());
        }
    };

    let (tx, mut rx) = mpsc::channel::<PathBuf>(1000);
    let sedang_diproses: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
    let sedang_diproses_watcher = Arc::clone(&sedang_diproses);

    // Pindai berkas yang belum berformat benar di seluruh folder klien
    let tx_scan = tx.clone();
    let sedang_diproses_scan = Arc::clone(&sedang_diproses);
    tokio::spawn(async move {
        pindai_folder_awal(Path::new(ROOT_FOLDER), &tx_scan, &sedang_diproses_scan).await;
    });

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                    for path in event.paths {
                        if is_folder_dikecualikan(&path) {
                            continue;
                        }
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            let lower = name.to_lowercase();
                            // Proses semua file .pdf yang namanya BELUM termasuk dalam 10 nama standar
                            let sudah_benar = NAMA_SUDAH_BENAR.iter().any(|&b| {
                                lower == b || lower.starts_with(&format!("{}_", b.trim_end_matches(".pdf")))
                            });

                            if lower.ends_with(".pdf") && !sudah_benar {
                                let mut set = sedang_diproses_watcher.lock().unwrap();
                                if !set.contains(&path) {
                                    set.insert(path.clone());
                                    let _ = tx.blocking_send(path);
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

    println!("======================================================");
    println!(" AUTO-RENAME NOTARIS AKTIF (NATIVE WINRT PDF & OCR) ");
    println!(" Memantau Folder : {}", ROOT_FOLDER);
    println!(" Pengecualian    : Folder tidak kapital semua dilewati (finish, revisi, old data, dll)");
    println!(" Memeriksa seluruh folder aktif dan mengubah nama file...");
    println!("======================================================");

    while let Some(path) = rx.recv().await {
        proses_satu_berkas(&path, &ocr_engine).await;
        sedang_diproses.lock().unwrap().remove(&path);
    }

    Ok(())
}