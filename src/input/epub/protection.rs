use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};

use anyhow::{Context, Result, bail};
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;
use zip8::ZipArchive;

mod media;
mod path;

use media::{is_font_core_media_type, is_font_obfuscation_algorithm};
use path::{ManifestLocation, normalize_archive_path, normalize_manifest_location};

const MAX_MANIFEST_RESOURCES: usize = 10_000;
const MAX_PACKAGE_BYTES: u64 = 32 * 1_024 * 1_024;
const OCF_NAMESPACE: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:container";
const OPF_NAMESPACE: &[u8] = b"http://www.idpf.org/2007/opf";
const XML_ENCRYPTION_NAMESPACE: &[u8] = b"http://www.w3.org/2001/04/xmlenc#";

#[derive(Default)]
struct EncryptionState {
    saw_algorithm: bool,
    font_algorithm: bool,
    protected: bool,
    uri: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EncryptionElement {
    EncryptedData,
    CipherData,
    Other,
}

pub(super) fn font_obfuscation_references(manifest: &[u8]) -> Result<Vec<String>> {
    let mut reader = NsReader::from_reader(manifest);
    let mut stack = Vec::new();
    let mut encrypted_resource = None;
    let mut references = Vec::new();

    loop {
        let (namespace, event) = reader
            .read_resolved_event()
            .context("failed to parse EPUB encryption manifest")?;
        match event {
            Event::Start(element) => {
                process_encryption_element(
                    &namespace,
                    &element,
                    stack.last().copied(),
                    &mut encrypted_resource,
                )?;
                stack.push(encryption_element_kind(
                    &namespace,
                    &element,
                    stack.last().copied(),
                ));
            }
            Event::Empty(element) => {
                if is_element(
                    &namespace,
                    &element,
                    XML_ENCRYPTION_NAMESPACE,
                    b"EncryptedData",
                ) {
                    bail!("Unsupported encrypted/DRM-protected input.");
                }
                process_encryption_element(
                    &namespace,
                    &element,
                    stack.last().copied(),
                    &mut encrypted_resource,
                )?;
            }
            Event::End(element) => {
                if namespace_is(&namespace, XML_ENCRYPTION_NAMESPACE)
                    && element.local_name().as_ref() == b"EncryptedData"
                {
                    finish_encrypted_resource(&mut encrypted_resource, &mut references)?;
                }
                stack
                    .pop()
                    .context("failed to parse EPUB encryption manifest: unmatched end tag")?;
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if encrypted_resource.is_some() || !stack.is_empty() {
        bail!("failed to parse EPUB encryption manifest: unclosed element");
    }
    Ok(references)
}

fn process_encryption_element(
    namespace: &ResolveResult<'_>,
    element: &BytesStart<'_>,
    parent: Option<EncryptionElement>,
    encrypted_resource: &mut Option<EncryptionState>,
) -> Result<()> {
    if is_element(
        namespace,
        element,
        XML_ENCRYPTION_NAMESPACE,
        b"EncryptedData",
    ) {
        if encrypted_resource.is_some() {
            bail!("failed to parse EPUB encryption manifest: nested EncryptedData");
        }
        *encrypted_resource = Some(EncryptionState::default());
        return Ok(());
    }
    if is_element(
        namespace,
        element,
        XML_ENCRYPTION_NAMESPACE,
        b"EncryptionMethod",
    ) && parent == Some(EncryptionElement::EncryptedData)
    {
        if let Some(state) = encrypted_resource {
            state.saw_algorithm = true;
            let algorithm = attribute_value(element, b"Algorithm")?;
            let is_font = algorithm
                .as_deref()
                .is_some_and(is_font_obfuscation_algorithm);
            state.font_algorithm |= is_font;
            state.protected |= !is_font;
        }
        return Ok(());
    }
    if is_element(
        namespace,
        element,
        XML_ENCRYPTION_NAMESPACE,
        b"CipherReference",
    ) && parent == Some(EncryptionElement::CipherData)
        && let Some(state) = encrypted_resource
    {
        let uri = attribute_value(element, b"URI")?;
        if state.uri.is_some() || uri.is_none() {
            state.protected = true;
        } else {
            state.uri = uri;
        }
    }
    Ok(())
}

fn encryption_element_kind(
    namespace: &ResolveResult<'_>,
    element: &BytesStart<'_>,
    parent: Option<EncryptionElement>,
) -> EncryptionElement {
    if is_element(
        namespace,
        element,
        XML_ENCRYPTION_NAMESPACE,
        b"EncryptedData",
    ) {
        EncryptionElement::EncryptedData
    } else if parent == Some(EncryptionElement::EncryptedData)
        && is_element(namespace, element, XML_ENCRYPTION_NAMESPACE, b"CipherData")
    {
        EncryptionElement::CipherData
    } else {
        EncryptionElement::Other
    }
}

fn finish_encrypted_resource(
    encrypted_resource: &mut Option<EncryptionState>,
    references: &mut Vec<String>,
) -> Result<()> {
    let state = encrypted_resource
        .take()
        .context("failed to parse EPUB encryption manifest: unmatched EncryptedData")?;
    if !state.saw_algorithm || !state.font_algorithm || state.protected {
        bail!("Unsupported encrypted/DRM-protected input.");
    }
    references.push(
        state
            .uri
            .context("Unsupported encrypted/DRM-protected input.")?,
    );
    if references.len() > MAX_MANIFEST_RESOURCES {
        bail!("EPUB encryption manifest contains too many resources");
    }
    Ok(())
}

pub(super) fn validate_font_references<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    container: &[u8],
    archive_names: &HashSet<String>,
    references: &[String],
) -> Result<()> {
    let package_paths = package_document_paths(container)?;
    let mut manifest_resources = HashMap::new();
    for package_path in package_paths {
        if !archive_names.contains(&package_path) {
            bail!("Unsupported encrypted/DRM-protected input.");
        }
        let package = read_archive_entry(archive, &package_path)?;
        collect_manifest_resources(&package, &package_path, &mut manifest_resources)?;
    }

    for reference in references {
        let Some(path) = normalize_archive_path(None, reference) else {
            bail!("Unsupported encrypted/DRM-protected input.");
        };
        if !archive_names.contains(&path)
            || manifest_resources.get(&path) != Some(&ManifestResourceKind::Font)
        {
            bail!("Unsupported encrypted/DRM-protected input.");
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContainerElement {
    Container,
    Rootfiles,
    Other,
}

fn package_document_paths(container: &[u8]) -> Result<Vec<String>> {
    let mut reader = NsReader::from_reader(container);
    let mut stack = Vec::new();
    let mut paths = Vec::new();
    let mut seen_paths = HashSet::new();
    loop {
        let (namespace, event) = reader
            .read_resolved_event()
            .context("failed to parse EPUB container metadata")?;
        match event {
            Event::Start(element) => {
                collect_rootfile(
                    &namespace,
                    &element,
                    stack.last().copied(),
                    &mut paths,
                    &mut seen_paths,
                )?;
                stack.push(container_element_kind(
                    &namespace,
                    &element,
                    stack.last().copied(),
                ));
            }
            Event::Empty(element) => collect_rootfile(
                &namespace,
                &element,
                stack.last().copied(),
                &mut paths,
                &mut seen_paths,
            )?,
            Event::End(_) => {
                stack
                    .pop()
                    .context("failed to parse EPUB container metadata: unmatched end tag")?;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if paths.is_empty() {
        bail!("EPUB container contains no package document");
    }
    Ok(paths)
}

fn collect_rootfile(
    namespace: &ResolveResult<'_>,
    element: &BytesStart<'_>,
    parent: Option<ContainerElement>,
    paths: &mut Vec<String>,
    seen_paths: &mut HashSet<String>,
) -> Result<()> {
    if parent != Some(ContainerElement::Rootfiles)
        || !is_element(namespace, element, OCF_NAMESPACE, b"rootfile")
    {
        return Ok(());
    }
    let full_path = attribute_value(element, b"full-path")?
        .context("EPUB container rootfile is missing full-path")?;
    let path = normalize_archive_path(None, &full_path)
        .context("EPUB container has an unsafe rootfile path")?;
    if !seen_paths.insert(path.clone()) {
        bail!("EPUB container contains a duplicate package document path");
    }
    paths.push(path);
    if paths.len() > MAX_MANIFEST_RESOURCES {
        bail!("EPUB container references too many package documents");
    }
    Ok(())
}

fn container_element_kind(
    namespace: &ResolveResult<'_>,
    element: &BytesStart<'_>,
    parent: Option<ContainerElement>,
) -> ContainerElement {
    if parent.is_none() && is_element(namespace, element, OCF_NAMESPACE, b"container") {
        ContainerElement::Container
    } else if parent == Some(ContainerElement::Container)
        && is_element(namespace, element, OCF_NAMESPACE, b"rootfiles")
    {
        ContainerElement::Rootfiles
    } else {
        ContainerElement::Other
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PackageElement {
    Package,
    Manifest,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ManifestResourceKind {
    Font,
    Other,
}

fn collect_manifest_resources(
    package: &[u8],
    package_path: &str,
    resources: &mut HashMap<String, ManifestResourceKind>,
) -> Result<()> {
    let mut reader = NsReader::from_reader(package);
    let mut stack = Vec::new();
    let mut manifest_urls = HashSet::new();
    let base = package_path
        .rsplit_once('/')
        .map(|(directory, _)| directory);
    loop {
        let (namespace, event) = reader
            .read_resolved_event()
            .context("failed to parse EPUB package document")?;
        match event {
            Event::Start(element) => {
                collect_manifest_item(
                    &namespace,
                    &element,
                    stack.last().copied(),
                    base,
                    &mut manifest_urls,
                    resources,
                )?;
                stack.push(package_element_kind(
                    &namespace,
                    &element,
                    stack.last().copied(),
                ));
            }
            Event::Empty(element) => collect_manifest_item(
                &namespace,
                &element,
                stack.last().copied(),
                base,
                &mut manifest_urls,
                resources,
            )?,
            Event::End(_) => {
                stack
                    .pop()
                    .context("failed to parse EPUB package document: unmatched end tag")?;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

fn collect_manifest_item(
    namespace: &ResolveResult<'_>,
    element: &BytesStart<'_>,
    parent: Option<PackageElement>,
    base: Option<&str>,
    manifest_urls: &mut HashSet<ManifestLocation>,
    resources: &mut HashMap<String, ManifestResourceKind>,
) -> Result<()> {
    if parent != Some(PackageElement::Manifest)
        || !is_element(namespace, element, OPF_NAMESPACE, b"item")
    {
        return Ok(());
    }
    let href = attribute_value(element, b"href")?.context("EPUB manifest item is missing href")?;
    let media_type = attribute_value(element, b"media-type")?
        .context("EPUB manifest item is missing media-type")?;
    let location = normalize_manifest_location(base, &href)
        .context("EPUB manifest item has an unsafe href")?;
    if !manifest_urls.insert(location.clone()) {
        bail!("EPUB package contains a duplicate manifest URL");
    }
    if manifest_urls.len() > MAX_MANIFEST_RESOURCES {
        bail!("EPUB package contains too many manifest resources");
    }
    let ManifestLocation::Archive(path) = location else {
        return Ok(());
    };
    let kind = if is_font_core_media_type(&media_type) {
        ManifestResourceKind::Font
    } else {
        ManifestResourceKind::Other
    };
    if resources
        .get(&path)
        .is_some_and(|existing| *existing != kind)
    {
        bail!("Unsupported encrypted/DRM-protected input.");
    }
    resources.insert(path, kind);
    if resources.len() > MAX_MANIFEST_RESOURCES {
        bail!("EPUB package contains too many manifest resources");
    }
    Ok(())
}

fn package_element_kind(
    namespace: &ResolveResult<'_>,
    element: &BytesStart<'_>,
    parent: Option<PackageElement>,
) -> PackageElement {
    if parent.is_none() && is_element(namespace, element, OPF_NAMESPACE, b"package") {
        PackageElement::Package
    } else if parent == Some(PackageElement::Package)
        && is_element(namespace, element, OPF_NAMESPACE, b"manifest")
    {
        PackageElement::Manifest
    } else {
        PackageElement::Other
    }
}

fn read_archive_entry<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>> {
    let mut entry = archive
        .by_name(name)
        .with_context(|| format!("failed to read EPUB package document {name}"))?;
    let mut bytes = Vec::new();
    entry
        .by_ref()
        .take(MAX_PACKAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read EPUB package document {name}"))?;
    if u64::try_from(bytes.len())? > MAX_PACKAGE_BYTES {
        bail!("EPUB package document exceeds 32 MiB limit");
    }
    Ok(bytes)
}

fn is_element(
    namespace: &ResolveResult<'_>,
    element: &BytesStart<'_>,
    expected_namespace: &[u8],
    local_name: &[u8],
) -> bool {
    namespace_is(namespace, expected_namespace) && element.local_name().as_ref() == local_name
}

fn namespace_is(namespace: &ResolveResult<'_>, expected: &[u8]) -> bool {
    matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == expected)
}

fn attribute_value(element: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>> {
    let Some(attribute) = element
        .try_get_attribute(name)
        .context("failed to parse EPUB XML attribute")?
    else {
        return Ok(None);
    };
    let raw =
        std::str::from_utf8(attribute.value.as_ref()).context("EPUB XML attribute is not UTF-8")?;
    Ok(Some(
        quick_xml::escape::unescape(raw)
            .context("failed to decode EPUB XML attribute")?
            .into_owned(),
    ))
}
