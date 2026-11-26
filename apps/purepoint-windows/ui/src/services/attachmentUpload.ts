// Port of apps/purepoint-macos/purepoint-macos/Services/AttachmentUpload.swift (WIN-9.7).
//
// Sends a file to an agent's daemon so the agent can read it by path, even when it runs on
// another machine. The daemon keeps it in `~/.pu/attachments` there. Files come from the
// picker (`<input type="file">`), a drop (the window's native drag-drop is off, so WebView2
// hands the page `File`s) or a paste (a clipboard image with no text).
//
// Windows: the Mac's Command-only paste check is Ctrl-only (`isCtrlOnly`); Caps Lock is not a
// modifier on a KeyboardEvent, so only Shift, Alt and the Win key disqualify it.
import type { Request, Response } from '../protocol';
import { bytesToBase64 } from '../model/piChatDomain';
import { daemonRequest } from './daemon';

/** Raw bytes per `attach_file` request: base64 keeps each well under the daemon's 1 MB limit. */
export const CHUNK_SIZE = 512 * 1024;
/** Matches the daemon's own limit. */
export const MAX_BYTES = 64 * 1024 * 1024;

export type AttachFileRequest = Extract<Request, { type: 'attach_file' }>;

export type UploadErrorKind = 'tooLarge' | 'notAFile' | 'daemon' | 'unexpectedResponse';

export class AttachmentUploadError extends Error {
  constructor(
    readonly kind: UploadErrorKind,
    message: string,
    readonly code: string | null = null,
  ) {
    super(message);
  }
  static tooLarge(name: string): AttachmentUploadError {
    return new AttachmentUploadError('tooLarge', `${name} is larger than 64 MB`);
  }
  static notAFile(name: string): AttachmentUploadError {
    return new AttachmentUploadError('notAFile', `${name} is not a regular file`);
  }
}

/** The `attach_file` requests that carry `data`, in order. */
export function attachFileRequests(
  data: Uint8Array,
  name: string,
  uploadId: string,
): AttachFileRequest[] {
  if (data.length === 0)
    return [
      { type: 'attach_file', upload_id: uploadId, name, offset: 0, data_base64: '', done: true },
    ];
  const requests: AttachFileRequest[] = [];
  for (let offset = 0; offset < data.length; offset += CHUNK_SIZE) {
    const end = Math.min(offset + CHUNK_SIZE, data.length);
    requests.push({
      type: 'attach_file',
      upload_id: uploadId,
      name,
      offset,
      data_base64: bytesToBase64(data.subarray(offset, end)),
      done: end === data.length,
    });
  }
  return requests;
}

/** The daemon's reply to one chunk: the path once finished, null while unfinished. */
export function attachFileResult(response: Response): string | null {
  switch (response.type) {
    case 'attach_file_result':
      return response.path;
    case 'error':
      throw new AttachmentUploadError('daemon', response.message, response.code);
    default:
      throw new AttachmentUploadError(
        'unexpectedResponse',
        "The agent's daemon did not accept the file",
      );
  }
}

/** Uploads `data` and returns its path on the daemon's machine. */
export async function uploadAttachment(
  data: Uint8Array,
  name: string,
  send: (request: Request) => Promise<Response> = daemonRequest,
  uploadId: string = crypto.randomUUID().toLowerCase(),
): Promise<string> {
  if (data.length > MAX_BYTES) throw AttachmentUploadError.tooLarge(name);
  for (const request of attachFileRequests(data, name, uploadId)) {
    const path = attachFileResult(await send(request));
    if (path !== null) return path;
  }
  throw new AttachmentUploadError(
    'unexpectedResponse',
    "The agent's daemon did not accept the file",
  );
}

/** A dropped or picked file: its bytes on demand, and whether the drop was a folder. */
export interface DroppedFile {
  readonly name: string;
  readonly size: number;
  arrayBuffer(): Promise<ArrayBuffer>;
}

/**
 * Reads a dropped file for upload. Checks it is a regular file within `MAX_BYTES` before
 * reading, so a folder or an oversized file is rejected without loading it.
 */
export async function readDroppedFile(file: DroppedFile, isDirectory = false): Promise<Uint8Array> {
  if (isDirectory) throw AttachmentUploadError.notAFile(file.name);
  if (file.size > MAX_BYTES) throw AttachmentUploadError.tooLarge(file.name);
  let buffer: ArrayBuffer;
  try {
    buffer = await file.arrayBuffer();
  } catch {
    // A folder dropped as a File, or a file removed since the drop, cannot be read.
    throw AttachmentUploadError.notAFile(file.name);
  }
  if (buffer.byteLength > MAX_BYTES) throw AttachmentUploadError.tooLarge(file.name);
  return new Uint8Array(buffer);
}

/** The files of a drop, each with whether it is a folder (`webkitGetAsEntry`). */
export function droppedFiles(data: DataTransfer): { file: File; isDirectory: boolean }[] {
  const files: { file: File; isDirectory: boolean }[] = [];
  for (const item of Array.from(data.items)) {
    if (item.kind !== 'file') continue;
    const file = item.getAsFile();
    if (file === null) continue;
    files.push({ file, isDirectory: item.webkitGetAsEntry()?.isDirectory === true });
  }
  return files;
}

/** Whether a key event's modifiers are Ctrl alone (the Mac's Command-only paste check). */
export function isCtrlOnly(event: {
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}): boolean {
  return event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey;
}

const IMAGE_EXTENSIONS: Record<string, string> = {
  'image/png': 'png',
  'image/jpeg': 'jpg',
  'image/gif': 'gif',
  'image/webp': 'webp',
  'image/bmp': 'bmp',
};

/**
 * A pasted image, when the clipboard holds an image and no text (a screenshot copied to the
 * clipboard, or "Copy image").
 */
export function pastedImage(data: DataTransfer | null): File | null {
  if (data === null || data.types.includes('text/plain')) return null;
  for (const item of Array.from(data.items)) {
    if (item.kind === 'file' && item.type.startsWith('image/')) return item.getAsFile();
  }
  return null;
}

const pad = (value: number) => String(value).padStart(2, '0');

/** A readable, unique-enough name for a pasted image. */
export function pastedImageName(date: Date = new Date(), mimeType = 'image/png'): string {
  const day = `${String(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  const time = `${pad(date.getHours())}.${pad(date.getMinutes())}.${pad(date.getSeconds())}`;
  return `Pasted Image ${day} at ${time}.${IMAGE_EXTENSIONS[mimeType] ?? 'png'}`;
}
