// Port of apps/purepoint-macos/purepoint-macos/Services/PiAttachmentImport.swift (WIN-9.7):
// prepares a picked, dropped or pasted file for the Point Guard composer with the iOS
// composer's limits. Images are re-encoded as JPEG within 256 KiB (longest side 1600, 1200
// or 800 px; quality 0.8, 0.6 or 0.4); text files must be UTF-8 within 32 KiB.
//
// Windows: the image pipeline is `createImageBitmap` and a canvas (behind `ImagePreparer`,
// which tests replace). PDFs are refused: the Mac extracts their text with PDFKit, and
// WebView2 has no text extraction short of bundling pdf.js.
import {
  bytesToBase64,
  PiComposerError,
  prefixCharacters,
  type PiComposerAttachment,
} from '../model/piChatDomain';

export const FILE_LIMIT = 20 * 1024 * 1024;
export const TEXT_LIMIT = 32 * 1024;
export const IMAGE_LIMIT = 256 * 1024;
const DIMENSIONS = [1600, 1200, 800];
const QUALITIES = [0.8, 0.6, 0.4];

/** A decoded image that can be re-encoded as JPEG at a size. */
export interface PreparedImage {
  /** A JPEG whose longest side is at most `maxDimension`, or null when encoding failed. */
  jpeg(maxDimension: number, quality: number): Promise<Blob | null>;
  close(): void;
}

/** Decodes images; null when the data is not an image it can open. */
export type ImagePreparer = (data: Blob) => Promise<PreparedImage | null>;

/** The WebView's decoder and a canvas. EXIF orientation is applied, as the Mac's thumbnail does. */
export const canvasImagePreparer: ImagePreparer = async (data) => {
  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(data, { imageOrientation: 'from-image' });
  } catch {
    return null;
  }
  return {
    jpeg: async (maxDimension, quality) => {
      const scale = Math.min(1, maxDimension / Math.max(bitmap.width, bitmap.height));
      const width = Math.max(1, Math.round(bitmap.width * scale));
      const height = Math.max(1, Math.round(bitmap.height * scale));
      const canvas = new OffscreenCanvas(width, height);
      const context = canvas.getContext('2d');
      if (context === null) return null;
      // JPEG has no alpha: flatten transparent images onto white.
      context.fillStyle = '#fff';
      context.fillRect(0, 0, width, height);
      context.drawImage(bitmap, 0, 0, width, height);
      try {
        return await canvas.convertToBlob({ type: 'image/jpeg', quality });
      } catch {
        return null;
      }
    },
    close: () => {
      bitmap.close();
    },
  };
};

const IMAGE_NAME = /\.(png|jpe?g|gif|webp|bmp|heic|heif|tiff?|avif)$/i;

export function isImageFile(file: { name: string; type: string }): boolean {
  return file.type.startsWith('image/') || (file.type === '' && IMAGE_NAME.test(file.name));
}

export function isPdfFile(file: { name: string; type: string }): boolean {
  return file.type === 'application/pdf' || /\.pdf$/i.test(file.name);
}

/** Re-encode an image as a JPEG composer attachment within 256 KiB. */
export async function importImage(
  data: Blob,
  name: string,
  prepare: ImagePreparer = canvasImagePreparer,
  uuid: () => string = () => crypto.randomUUID(),
): Promise<PiComposerAttachment> {
  if (data.size > FILE_LIMIT) throw new PiComposerError('Choose an image smaller than 20 MB.');
  const image = await prepare(data);
  if (image === null) throw new PiComposerError('This image could not be opened.');
  try {
    for (const dimension of DIMENSIONS) {
      for (const quality of QUALITIES) {
        const jpeg = await image.jpeg(dimension, quality);
        if (jpeg === null) throw new PiComposerError('This image could not be prepared.');
        if (jpeg.size <= IMAGE_LIMIT) {
          const bytes = new Uint8Array(await jpeg.arrayBuffer());
          return { id: uuid(), name, mimeType: 'image/jpeg', data: bytesToBase64(bytes) };
        }
      }
    }
  } finally {
    image.close();
  }
  throw new PiComposerError('This image could not be prepared. Choose a smaller image.');
}

/** Prepare a picked, dropped or pasted file for the composer. */
export async function importAttachment(
  file: File,
  prepare: ImagePreparer = canvasImagePreparer,
  uuid: () => string = () => crypto.randomUUID(),
): Promise<PiComposerAttachment> {
  if (file.size > FILE_LIMIT) throw new PiComposerError('Choose a file smaller than 20 MB.');
  const name = prefixCharacters(file.name, 120);
  if (isImageFile(file)) return importImage(file, name, prepare, uuid);
  if (isPdfFile(file))
    throw new PiComposerError(
      'PDF attachments are not supported on Windows yet. Attach the text as a .txt file, or the pages as images.',
    );
  const failure = 'Choose a UTF-8 text file smaller than 32 KiB.';
  if (file.size > TEXT_LIMIT) throw new PiComposerError(failure);
  let text: string;
  try {
    text = new TextDecoder('utf-8', { fatal: true }).decode(await file.arrayBuffer());
  } catch {
    throw new PiComposerError(failure);
  }
  if (text.includes('\0')) throw new PiComposerError(failure);
  const bytes = new TextEncoder().encode(text);
  if (bytes.length > TEXT_LIMIT)
    throw new PiComposerError('The extracted text exceeds 32 KiB. Choose a shorter document.');
  return { id: uuid(), name, mimeType: 'text/plain', data: bytesToBase64(bytes) };
}

/**
 * Attach picked, dropped or pasted files to the composer: each is prepared and added in
 * order; the first failure is reported and stops the rest.
 */
export async function attachToComposer(
  files: readonly File[],
  composer: { addAttachment(file: PiComposerAttachment): void; error: string | null },
  prepare: ImagePreparer = canvasImagePreparer,
): Promise<void> {
  for (const file of files) {
    try {
      composer.addAttachment(await importAttachment(file, prepare));
    } catch (error) {
      composer.error = error instanceof Error ? error.message : String(error);
      return;
    }
  }
}
