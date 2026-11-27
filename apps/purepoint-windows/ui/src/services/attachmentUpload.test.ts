// Port of the platform-neutral AttachmentUploadTests (WIN-9.7). The Mac-only cases map as:
// Command-only paste → Ctrl-only (`isCtrlOnly`); a dropped URL → a dropped `File`. The attach
// session's undelivered-input case belongs to WIN-5.4's attach tests.
import { describe, expect, it, vi } from 'vitest';
import type { Request, Response } from '../protocol';
import { base64ToBytes } from '../model/piChatDomain';
import {
  attachFileRequests,
  attachFileResult,
  AttachmentUploadError,
  CHUNK_SIZE,
  isCtrlOnly,
  MAX_BYTES,
  pastedImage,
  pastedImageName,
  readDroppedFile,
  uploadAttachment,
} from './attachmentUpload';

const keys = (patch: Partial<Record<'ctrlKey' | 'shiftKey' | 'altKey' | 'metaKey', boolean>>) => ({
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  metaKey: false,
  ...patch,
});

describe('AttachmentUpload', () => {
  it('given an attach_file request should encode the chunk as base64', () => {
    const [request] = attachFileRequests(new TextEncoder().encode('hi'), 'shot.png', 'u1');
    expect(request).toEqual({
      type: 'attach_file',
      upload_id: 'u1',
      name: 'shot.png',
      offset: 0,
      data_base64: 'aGk=',
      done: true,
    });
  });

  it('given an attach_file result should decode the path only when finished', () => {
    expect(attachFileResult({ type: 'attach_file_result', received: 6, path: null })).toBeNull();
    expect(
      attachFileResult({
        type: 'attach_file_result',
        received: 6,
        path: '/h/.pu/attachments/u1/a.png',
      }),
    ).toBe('/h/.pu/attachments/u1/a.png');
    expect(() => attachFileResult({ type: 'error', code: 'too_large', message: 'nope' })).toThrow(
      'nope',
    );
    expect(() => attachFileResult({ type: 'ok' })).toThrow('did not accept the file');
  });

  it('given data larger than one chunk should split in order and finish on the last', () => {
    const data = new Uint8Array(CHUNK_SIZE * 2 + 10).fill(7);
    const requests = attachFileRequests(data, 'a.bin', 'u1');
    expect(requests.map((r) => r.offset)).toEqual([0, CHUNK_SIZE, CHUNK_SIZE * 2]);
    expect(requests.map((r) => r.done)).toEqual([false, false, true]);
    expect(requests.map((r) => base64ToBytes(r.data_base64).length)).toEqual([
      CHUNK_SIZE,
      CHUNK_SIZE,
      10,
    ]);
  });

  it('given an empty file should send one finished chunk', () => {
    const requests = attachFileRequests(new Uint8Array(), 'empty', 'u1');
    expect(requests).toHaveLength(1);
    expect(requests[0]?.done).toBe(true);
  });

  it('given an upload should send every chunk and return the finished path', async () => {
    const sent: Request[] = [];
    const send = (request: Request): Promise<Response> => {
      sent.push(request);
      const done = request.type === 'attach_file' && request.done === true;
      return Promise.resolve({
        type: 'attach_file_result',
        received: sent.length,
        path: done ? 'C:\\Users\\a\\.pu\\attachments\\u1\\a.bin' : null,
      });
    };
    const path = await uploadAttachment(new Uint8Array(CHUNK_SIZE + 1), 'a.bin', send, 'u1');
    expect(path).toBe('C:\\Users\\a\\.pu\\attachments\\u1\\a.bin');
    expect(sent).toHaveLength(2);
  });

  it('given a daemon error should stop the upload with its message', async () => {
    const send = vi.fn(() =>
      Promise.resolve<Response>({ type: 'error', code: 'io', message: 'disk full' }),
    );
    const error = await uploadAttachment(new Uint8Array(CHUNK_SIZE * 2), 'a', send, 'u').catch(
      (e: unknown) => e,
    );
    expect(error).toBeInstanceOf(AttachmentUploadError);
    expect(error).toMatchObject({ kind: 'daemon', code: 'io', message: 'disk full' });
    expect(send).toHaveBeenCalledTimes(1);
  });

  it('given Ctrl+V with Caps Lock should still be a paste', () => {
    expect(isCtrlOnly(keys({ ctrlKey: true }))).toBe(true);
  });

  it('given Ctrl with another shortcut modifier should not be a paste', () => {
    expect(isCtrlOnly(keys({ ctrlKey: true, shiftKey: true }))).toBe(false);
    expect(isCtrlOnly(keys({ ctrlKey: true, altKey: true }))).toBe(false);
    expect(isCtrlOnly(keys({ ctrlKey: true, metaKey: true }))).toBe(false);
    expect(isCtrlOnly(keys({}))).toBe(false);
  });

  it('given a dropped regular file should read its bytes', async () => {
    const file = new File(['hello'], 'a.txt');
    expect(new TextDecoder().decode(await readDroppedFile(file))).toBe('hello');
  });

  it('given a dropped folder should reject it', async () => {
    const folder = new File([], 'Folder');
    await expect(readDroppedFile(folder, true)).rejects.toMatchObject({ kind: 'notAFile' });
  });

  it('given a dropped file over the limit should reject it without reading', async () => {
    const arrayBuffer = vi.fn(() => Promise.resolve(new ArrayBuffer(0)));
    const big = { name: 'big.bin', size: MAX_BYTES + 1, arrayBuffer };
    await expect(readDroppedFile(big)).rejects.toMatchObject({ kind: 'tooLarge' });
    expect(arrayBuffer).not.toHaveBeenCalled();
    await expect(uploadAttachment(new Uint8Array(MAX_BYTES + 1), 'big')).rejects.toMatchObject({
      kind: 'tooLarge',
    });
  });

  it('given a missing file should throw', async () => {
    const gone = {
      name: 'gone',
      size: 1,
      arrayBuffer: () => Promise.reject(new DOMException('gone', 'NotFoundError')),
    };
    await expect(readDroppedFile(gone)).rejects.toBeInstanceOf(AttachmentUploadError);
  });

  it('given a clipboard should take an image only when it holds no text', () => {
    const image = new File([new Uint8Array([1])], 'image.png', { type: 'image/png' });
    const clipboard = (types: string[]) =>
      ({
        types,
        items: [{ kind: 'file', type: 'image/png', getAsFile: () => image }],
      }) as unknown as DataTransfer;
    expect(pastedImage(clipboard(['Files']))).toBe(image);
    expect(pastedImage(clipboard(['text/plain', 'Files']))).toBeNull();
    expect(pastedImage(null)).toBeNull();
  });

  it('given a paste time should name the image readably', () => {
    const date = new Date(2026, 10, 26, 9, 5, 3);
    expect(pastedImageName(date)).toBe('Pasted Image 2026-11-26 at 09.05.03.png');
    expect(pastedImageName(date, 'image/jpeg')).toBe('Pasted Image 2026-11-26 at 09.05.03.jpg');
  });
});
