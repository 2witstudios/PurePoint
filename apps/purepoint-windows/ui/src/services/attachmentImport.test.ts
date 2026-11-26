// PiAttachmentImport (WIN-9.7): composer limits for picked, dropped and pasted files. The
// image encoder is scripted; the WebView's canvas pipeline is not available under jsdom.
import { describe, expect, it, vi } from 'vitest';
import { base64ToBytes } from '../model/piChatDomain';
import {
  attachToComposer,
  IMAGE_LIMIT,
  importAttachment,
  type ImagePreparer,
} from './attachmentImport';

/** An encoder whose JPEG size at each (dimension, quality) step comes from `sizes`. */
function scripted(sizes: number[], calls: [number, number][] = []): ImagePreparer {
  return () =>
    Promise.resolve({
      jpeg: (dimension: number, quality: number) => {
        calls.push([dimension, quality]);
        const size = sizes.shift() ?? IMAGE_LIMIT + 1;
        return Promise.resolve(new Blob([new Uint8Array(size).fill(0xff)], { type: 'image/jpeg' }));
      },
      close: () => {},
    });
}

const uuid = () => 'id-1';

describe('importAttachment', () => {
  it('given a UTF-8 text file should attach its text', async () => {
    const file = new File(['notes ✓'], 'notes.md', { type: 'text/markdown' });
    const attachment = await importAttachment(file, scripted([]), uuid);
    expect(attachment).toMatchObject({ id: 'id-1', name: 'notes.md', mimeType: 'text/plain' });
    expect(new TextDecoder().decode(base64ToBytes(attachment.data))).toBe('notes ✓');
  });

  it('given a binary, NUL-holding or long text file should refuse it', async () => {
    const message = 'Choose a UTF-8 text file smaller than 32 KiB.';
    await expect(
      importAttachment(new File([new Uint8Array([0xff, 0xfe])], 'a.bin')),
    ).rejects.toThrow(message);
    await expect(importAttachment(new File(['a\0b'], 'a.txt'))).rejects.toThrow(message);
    await expect(importAttachment(new File(['x'.repeat(32 * 1024 + 1)], 'a.txt'))).rejects.toThrow(
      message,
    );
  });

  it('given a file over 20 MB should refuse it without reading', async () => {
    const big = { name: 'big.txt', type: 'text/plain', size: 20 * 1024 * 1024 + 1 } as File;
    await expect(importAttachment(big)).rejects.toThrow('Choose a file smaller than 20 MB.');
  });

  it('given an image should step down size and quality until it fits 256 KiB', async () => {
    const calls: [number, number][] = [];
    const image = new File([new Uint8Array(10)], 'photo.heic', { type: '' });
    const attachment = await importAttachment(
      image,
      scripted([IMAGE_LIMIT + 1, IMAGE_LIMIT + 1, IMAGE_LIMIT + 1, IMAGE_LIMIT], calls),
      uuid,
    );
    expect(calls).toEqual([
      [1600, 0.8],
      [1600, 0.6],
      [1600, 0.4],
      [1200, 0.8],
    ]);
    expect(attachment.mimeType).toBe('image/jpeg');
    expect(base64ToBytes(attachment.data)).toHaveLength(IMAGE_LIMIT);
  });

  it('given an image that never fits or cannot be opened should explain', async () => {
    const image = new File([new Uint8Array(10)], 'a.png', { type: 'image/png' });
    await expect(importAttachment(image, scripted([]))).rejects.toThrow(
      'This image could not be prepared. Choose a smaller image.',
    );
    await expect(importAttachment(image, () => Promise.resolve(null))).rejects.toThrow(
      'This image could not be opened.',
    );
  });

  it('given a PDF should refuse it with the Windows alternative', async () => {
    const pdf = new File(['%PDF-1.7'], 'paper.pdf', { type: 'application/pdf' });
    await expect(importAttachment(pdf)).rejects.toThrow('PDF attachments are not supported');
  });

  it('given a long name should keep its first 120 characters', async () => {
    const file = new File(['x'], `${'n'.repeat(130)}.txt`);
    expect((await importAttachment(file, scripted([]), uuid)).name).toHaveLength(120);
  });
});

describe('attachToComposer', () => {
  it('given picked files should add each, and report the first failure', async () => {
    const composer = { addAttachment: vi.fn(), error: null as string | null };
    await attachToComposer(
      [
        new File(['a'], 'a.txt'),
        new File([new Uint8Array([0xff])], 'b.bin'),
        new File(['c'], 'c.txt'),
      ],
      composer,
      scripted([]),
    );
    expect(composer.addAttachment).toHaveBeenCalledTimes(1);
    expect(composer.error).toBe('Choose a UTF-8 text file smaller than 32 KiB.');
  });
});
