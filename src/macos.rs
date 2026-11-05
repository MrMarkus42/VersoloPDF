//! macOS system functions, through Apple's own frameworks:
//!
//! * PDFs opened in the Finder (double click, "Öffnen mit", dropped on the
//!   Dock icon) arrive as an "open documents" Apple Event, also while
//!   VersoloPDF already runs.
//! * "Per E-Mail senden": a new message in Apple Mail (sharing service) or
//!   Microsoft Outlook with the files attached.
//! * Printing through the macOS print dialog (PDFKit).
//! * Text recognition with Apple's Vision framework (no extra program).
//! * Digital IDs (certificate + private key) from the keychain; the private
//!   key never leaves it, macOS signs and may ask the user first.
//!
//! All `unsafe` blocks call documented Objective-C / C functions of these
//! frameworks with valid objects and check the results.
#![allow(unsafe_code)]

use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::Mutex;

use objc2::rc::{Retained, autoreleasepool};
use objc2::{AllocAnyThread, ClassType, MainThreadMarker, define_class, msg_send, sel};
use objc2_foundation::{
    NSAppleEventDescriptor, NSAppleEventManager, NSArray, NSDictionary, NSError, NSNotification,
    NSNotificationCenter, NSNumber, NSObject, NSRange, NSString, NSURL,
};

// ---------------------------------------------------------------------------
// Files opened in the Finder

/// Files that arrived by Apple Event and were not taken yet.
static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
/// Repaints the window when files arrive.
static WAKER: Mutex<Option<egui::Context>> = Mutex::new(None);

/// Four-character codes of the Apple Event: class 'aevt', id 'odoc', the
/// direct parameter '----'.
const AE_CORE_CLASS: u32 = u32::from_be_bytes(*b"aevt");
const AE_OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
const KEY_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

define_class!(
    /// Receives "the program will finish launching" and the "open
    /// documents" Apple Events.
    #[unsafe(super(NSObject))]
    #[name = "VersoloPDFOpenHandler"]
    struct OpenHandler;

    impl OpenHandler {
        /// AppKit installs its own handlers before this notification and
        /// handles the first "open documents" event after it: replacing the
        /// handler here catches the files the program was started with.
        #[unsafe(method(versoloWillFinishLaunching:))]
        fn will_finish_launching(&self, _note: &NSNotification) {
            let manager = NSAppleEventManager::sharedAppleEventManager();
            // SAFETY: `self` lives for the whole program (never released) and
            // has the method named by the selector.
            let () = unsafe {
                msg_send![
                    &*manager,
                    setEventHandler: self,
                    andSelector: sel!(versoloOpenDocuments:withReply:),
                    forEventClass: AE_CORE_CLASS,
                    andEventID: AE_OPEN_DOCUMENTS
                ]
            };
        }

        #[unsafe(method(versoloOpenDocuments:withReply:))]
        fn open_documents(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            let files = files_of_event(event);
            if files.is_empty() {
                return;
            }
            if let Ok(mut list) = OPENED.lock() {
                list.extend(files);
            }
            if let Ok(waker) = WAKER.lock()
                && let Some(ctx) = waker.as_ref()
            {
                ctx.request_repaint();
            }
        }
    }
);

/// The file paths in an "open documents" event.
fn files_of_event(event: &NSAppleEventDescriptor) -> Vec<PathBuf> {
    // SAFETY: '----' is a valid keyword; the result is checked.
    let list: Option<Retained<NSAppleEventDescriptor>> =
        unsafe { msg_send![event, paramDescriptorForKeyword: KEY_DIRECT_OBJECT] };
    let Some(list) = list else { return Vec::new() };
    let mut files = Vec::new();
    // Lists count from 1.
    for i in 1..=list.numberOfItems().min(200) {
        if let Some(path) = list.descriptorAtIndex(i).and_then(|d| d.fileURLValue()).and_then(|u| u.path()) {
            files.push(PathBuf::from(path.to_string()));
        }
    }
    files
}

/// Call once before the window is created: from then on files opened in
/// the Finder are collected for [`take_opened_files`].
pub fn install_open_handler() {
    // SAFETY: plain object creation.
    let handler: Retained<OpenHandler> = unsafe { msg_send![OpenHandler::class(), new] };
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: the handler has the method; the notification name is a
    // constant of AppKit. The handler is never released, so the
    // notification centre and the Apple Event manager may keep pointing at it.
    unsafe {
        center.addObserver_selector_name_object(
            &handler,
            sel!(versoloWillFinishLaunching:),
            Some(objc2_app_kit::NSApplicationWillFinishLaunchingNotification),
            None,
        );
    }
    std::mem::forget(handler);
}

/// Lets arriving files repaint the window.
pub fn set_waker(ctx: &egui::Context) {
    if let Ok(mut w) = WAKER.lock() {
        *w = Some(ctx.clone());
    }
}

/// Files opened in the Finder since the last call.
pub fn take_opened_files() -> Vec<PathBuf> {
    OPENED.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// E-mail

fn ns_string(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

fn file_url(path: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&ns_string(&path.to_string_lossy()))
}

/// The program macOS uses for `mailto:` addresses.
fn mail_program() -> Option<PathBuf> {
    let mailto = NSURL::URLWithString(&ns_string("mailto:"))?;
    let app = objc2_app_kit::NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&mailto)?;
    app.path().map(|p| PathBuf::from(p.to_string()))
}

/// A new message with `subject` and the `files` attached. Apple Mail gets
/// it through the sharing service, Outlook by opening the files with it.
/// `false`: no supported mail program (the caller falls back to `mailto:`).
pub fn compose_mail(subject: &str, files: &[PathBuf]) -> bool {
    if MainThreadMarker::new().is_none() {
        return false;
    }
    let program = mail_program();
    let name = program
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if name.contains("outlook") && !files.is_empty() {
        // Opening files with Outlook starts a message with them attached.
        let mut cmd = std::process::Command::new("/usr/bin/open");
        cmd.arg("-a").arg(program.unwrap_or_default());
        return cmd.args(files).status().is_ok_and(|s| s.success());
    }
    if !(name.is_empty() || name == "mail.app") {
        return false;
    }
    autoreleasepool(|_| {
        // SAFETY: constant of AppKit.
        let service_name = unsafe { objc2_app_kit::NSSharingServiceNameComposeEmail };
        let Some(service) = objc2_app_kit::NSSharingService::sharingServiceNamed(service_name) else {
            return false;
        };
        service.setSubject(Some(&ns_string(subject)));
        let urls: Vec<Retained<NSURL>> = files.iter().map(|f| file_url(f)).collect();
        let items = NSArray::from_retained_slice(&urls);
        // SAFETY: an array of file URLs is what the e-mail service takes.
        let can: bool = unsafe { msg_send![&*service, canPerformWithItems: &*items] };
        if !can {
            return false;
        }
        // SAFETY: as above.
        let () = unsafe { msg_send![&*service, performWithItems: &*items] };
        true
    })
}

// ---------------------------------------------------------------------------
// Printing

/// Shows the macOS print dialog for the PDF file `path` (pages already
/// laid out on sheets) with `printer` and `copies` preselected. `Ok(false)`:
/// the user cancelled.
pub fn print_pdf(path: &Path, printer: &str, copies: u32, title: &str) -> Result<bool, String> {
    use objc2_app_kit::{NSPrintCopies, NSPrintInfo, NSPrinter};
    use objc2_pdf_kit::{PDFDocument, PDFPrintScalingMode};
    let mtm = MainThreadMarker::new().ok_or("Drucken nur im Programmfenster möglich")?;
    autoreleasepool(|_| {
        // SAFETY: valid file URL; the result is checked.
        let doc = unsafe { PDFDocument::initWithURL(PDFDocument::alloc(), &file_url(path)) }
            .ok_or("Die Druckdatei konnte nicht gelesen werden.")?;
        // A copy, so the program-wide settings stay as they are.
        let shared = NSPrintInfo::sharedPrintInfo();
        // SAFETY: NSPrintInfo implements NSCopying; `copy` returns +1.
        let info: Retained<NSPrintInfo> = unsafe { msg_send![&*shared, copy] };
        if !printer.is_empty()
            && let Some(p) = NSPrinter::printerWithName(&ns_string(printer))
        {
            info.setPrinter(&p);
        }
        // SAFETY: NSPrintCopies takes an NSNumber.
        unsafe {
            let dict = info.dictionary();
            let number = NSNumber::new_u32(copies.clamp(1, 999));
            let () = msg_send![&*dict, setObject: &*number, forKey: NSPrintCopies];
        }
        // The sheets already have the paper's size: no further scaling.
        // SAFETY: valid print info; main thread (mtm).
        let op = unsafe {
            doc.printOperationForPrintInfo_scalingMode_autoRotate(
                Some(&info),
                PDFPrintScalingMode(0),
                false,
                mtm,
            )
        }
        .ok_or("macOS konnte den Druckauftrag nicht vorbereiten.")?;
        op.setShowsPrintPanel(true);
        op.setShowsProgressPanel(true);
        op.setJobTitle(Some(&ns_string(title)));
        Ok(op.runOperation())
    })
}

// ---------------------------------------------------------------------------
// Text recognition (Vision)

/// A recognised word: text, box in image coordinates normalised to 0..1
/// with the origin at the lower left (x, y, width, height), confidence 0..1.
pub struct VisionWord {
    pub text: String,
    pub rect: [f64; 4],
    pub confidence: f32,
}

/// Languages Vision recognises ("de-DE", "en-US", …).
pub fn vision_languages() -> Vec<String> {
    use objc2_vision::{VNRecognizeTextRequest, VNRequestTextRecognitionLevel};
    autoreleasepool(|_| {
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        // SAFETY: plain query.
        match unsafe { request.supportedRecognitionLanguagesAndReturnError() } {
            Ok(list) => list.iter().map(|s| s.to_string()).collect(),
            Err(_) => Vec::new(),
        }
    })
}

/// Recognises the words in the image file `image` (PNG).
pub fn recognize_text(image: &Path, languages: &[String]) -> Result<Vec<VisionWord>, String> {
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRectangleObservation, VNRequest,
        VNRequestTextRecognitionLevel,
    };
    autoreleasepool(|_| {
        let options = NSDictionary::new();
        // SAFETY: valid URL and empty options.
        let handler = unsafe {
            VNImageRequestHandler::initWithURL_options(
                VNImageRequestHandler::alloc(),
                &file_url(image),
                &options,
            )
        };
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        if !languages.is_empty() {
            let langs: Vec<Retained<NSString>> = languages.iter().map(|l| ns_string(l)).collect();
            request.setRecognitionLanguages(&NSArray::from_retained_slice(&langs));
        }
        let as_request: Retained<VNRequest> = Retained::into_super(Retained::into_super(request.clone()));
        handler
            .performRequests_error(&NSArray::from_retained_slice(&[as_request]))
            .map_err(|e: Retained<NSError>| e.localizedDescription().to_string())?;
        let mut words = Vec::new();
        for line in request.results().map(|r| r.to_vec()).unwrap_or_default() {
            let Some(best) = line.topCandidates(1).firstObject() else { continue };
            let text = best.string().to_string();
            let confidence = best.confidence();
            // SAFETY: plain property of the observation.
            let line_box = unsafe { line.boundingBox() };
            let total = text.encode_utf16().count().max(1) as f64;
            let mut offset = 0usize; // UTF-16 position
            for (piece, is_word) in split_words(&text) {
                let len = piece.encode_utf16().count();
                if is_word {
                    let range = NSRange::new(offset, len);
                    // SAFETY: the range lies inside the string.
                    let found: Result<Retained<VNRectangleObservation>, Retained<NSError>> =
                        unsafe { msg_send![&*best, boundingBoxForRange: range, error: _] };
                    let rect = match found {
                        // SAFETY: plain property of the observation.
                        Ok(obs) => {
                            let b = unsafe { obs.boundingBox() };
                            [b.origin.x, b.origin.y, b.size.width, b.size.height]
                        }
                        // Without a box: the matching part of the line.
                        Err(_) => [
                            line_box.origin.x + line_box.size.width * offset as f64 / total,
                            line_box.origin.y,
                            line_box.size.width * len as f64 / total,
                            line_box.size.height,
                        ],
                    };
                    words.push(VisionWord { text: piece.to_string(), rect, confidence });
                }
                offset += len;
            }
        }
        Ok(words)
    })
}

/// The text cut into words and the spaces between them.
fn split_words(text: &str) -> Vec<(&str, bool)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_word = None;
    for (i, c) in text.char_indices() {
        let word = !c.is_whitespace();
        if in_word != Some(word) {
            if let Some(w) = in_word {
                out.push((&text[start..i], w));
            }
            start = i;
            in_word = Some(word);
        }
    }
    if let Some(w) = in_word {
        out.push((&text[start..], w));
    }
    out
}

// ---------------------------------------------------------------------------
// Keychain

/// Certificates with a private key in the user's keychains: (name for the
/// list, certificate DER).
pub fn keychain_identities() -> Vec<(String, Vec<u8>)> {
    identities().into_iter().filter_map(|id| certificate_of(&id)).collect()
}

/// Signs `digest` (SHA-256) with the private key that belongs to the
/// certificate `cert_der`. `rsa`: PKCS#1 v1.5, else ECDSA (DER encoded).
pub fn keychain_sign(cert_der: &[u8], digest: &[u8], rsa: bool) -> Result<Vec<u8>, String> {
    use objc2_core_foundation::{CFData, CFRetained};
    use objc2_security::{
        SecKey, kSecKeyAlgorithmECDSASignatureDigestX962SHA256,
        kSecKeyAlgorithmRSASignatureDigestPKCS1v15SHA256,
    };
    let identity = identities()
        .into_iter()
        .find(|id| certificate_of(id).is_some_and(|(_, der)| der == cert_der))
        .ok_or("Die digitale ID ist nicht mehr im Schlüsselbund.")?;
    let mut key: *mut SecKey = std::ptr::null_mut();
    // SAFETY: valid identity; `key` receives a +1 reference or stays null.
    let status = unsafe { identity.copy_private_key(NonNull::from(&mut key)) };
    let key = NonNull::new(key)
        .filter(|_| status == 0)
        // SAFETY: +1 reference from the call above.
        .map(|k| unsafe { CFRetained::from_raw(k) })
        .ok_or_else(|| format!("Kein Zugriff auf den privaten Schlüssel (Fehler {status})."))?;
    // SAFETY: constants of the Security framework.
    let algorithm = unsafe {
        if rsa {
            kSecKeyAlgorithmRSASignatureDigestPKCS1v15SHA256
        } else {
            kSecKeyAlgorithmECDSASignatureDigestX962SHA256
        }
    };
    let data = CFData::from_bytes(digest);
    let mut error: *mut objc2_core_foundation::CFError = std::ptr::null_mut();
    // SAFETY: valid key, algorithm and data; `error` receives a +1 error.
    let signature = unsafe { key.signature(algorithm, &data, &mut error) };
    if let Some(err) = NonNull::new(error) {
        // SAFETY: +1 reference from the call above.
        let err = unsafe { CFRetained::from_raw(err) };
        return Err(format!("Signieren mit dem Schlüsselbund fehlgeschlagen: {err:?}"));
    }
    signature.map(|s| s.to_vec()).ok_or_else(|| "Signieren mit dem Schlüsselbund abgelehnt.".into())
}

fn identities() -> Vec<objc2_core_foundation::CFRetained<objc2_security::SecIdentity>> {
    use objc2_core_foundation::{CFArray, CFDictionary, CFRetained, CFString, CFType, kCFBooleanTrue};
    use objc2_security::{
        SecIdentity, SecItemCopyMatching, kSecClass, kSecClassIdentity, kSecMatchLimit, kSecMatchLimitAll,
        kSecReturnRef,
    };
    // SAFETY: constants of the Security / Core Foundation frameworks.
    let (keys, values): ([&CFString; 3], [&CFType; 3]) = unsafe {
        let Some(yes) = kCFBooleanTrue else { return Vec::new() };
        (
            [kSecClass, kSecReturnRef, kSecMatchLimit],
            [kSecClassIdentity.as_ref(), yes.as_ref(), kSecMatchLimitAll.as_ref()],
        )
    };
    let query = CFDictionary::from_slices(&keys, &values);
    let mut result: *const CFType = std::ptr::null();
    // SAFETY: valid query; `result` receives a +1 reference or stays null.
    let status = unsafe { SecItemCopyMatching(query.as_ref(), &mut result) };
    let Some(result) = NonNull::new(result.cast_mut()).filter(|_| status == 0) else { return Vec::new() };
    // SAFETY: +1 reference from the call above.
    let result: CFRetained<CFType> = unsafe { CFRetained::from_raw(result) };
    let Ok(array) = result.downcast::<CFArray>() else { return Vec::new() };
    // SAFETY: the array holds CF objects; each is checked below.
    let array: CFRetained<CFArray<CFType>> = unsafe { CFRetained::cast_unchecked(array) };
    array.to_vec().into_iter().filter_map(|item| item.downcast::<SecIdentity>().ok()).take(200).collect()
}

/// Name and DER of the certificate of `identity`.
fn certificate_of(identity: &objc2_security::SecIdentity) -> Option<(String, Vec<u8>)> {
    use objc2_core_foundation::CFRetained;
    use objc2_security::SecCertificate;
    let mut cert: *mut SecCertificate = std::ptr::null_mut();
    // SAFETY: valid identity; `cert` receives a +1 reference or stays null.
    let status = unsafe { identity.copy_certificate(NonNull::from(&mut cert)) };
    let cert = NonNull::new(cert).filter(|_| status == 0)?;
    // SAFETY: +1 reference from the call above.
    let cert = unsafe { CFRetained::from_raw(cert) };
    // SAFETY: valid certificate.
    let der = unsafe { cert.data() }.to_vec();
    // SAFETY: as above.
    let name = unsafe { cert.subject_summary() }.map(|s| s.to_string()).unwrap_or_default();
    Some((name, der))
}

#[cfg(test)]
mod tests {
    use super::split_words;

    #[test]
    fn words_and_spaces() {
        assert_eq!(split_words("Hallo  Welt"), vec![("Hallo", true), ("  ", false), ("Welt", true)]);
        assert!(split_words("").is_empty());
    }
}
