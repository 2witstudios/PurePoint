// The composer's attachment chip: image thumbnails are data: URLs, which the app CSP
// (`img-src 'self' data: https:`) allows; blob: URLs would be blocked.
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { attachmentThumbnailSrc, type PiComposerAttachment } from '../../model/piChatDomain.ts';
import { AttachmentChip } from './PiPointGuardChatView.tsx';

const png: PiComposerAttachment = {
  id: 'a',
  name: 'shot.png',
  mimeType: 'image/png',
  data: 'iVBORw0KGgo=',
};
const notes: PiComposerAttachment = {
  id: 'b',
  name: 'notes.txt',
  mimeType: 'text/plain',
  data: 'aGk=',
};

describe('AttachmentChip', () => {
  it('given an image should show a data: URL thumbnail built from its base64', () => {
    const { container } = render(<AttachmentChip file={png} onRemove={() => {}} />);
    const img = container.querySelector('img.pg-attachment-thumb');
    expect(img?.getAttribute('src')).toBe('data:image/png;base64,iVBORw0KGgo=');
    expect(img?.getAttribute('src')).not.toMatch(/^blob:/);
  });

  it('given a text file should show no thumbnail', () => {
    const { container } = render(<AttachmentChip file={notes} onRemove={() => {}} />);
    expect(container.querySelector('img')).toBeNull();
    expect(screen.getByText('notes.txt')).toBeInTheDocument();
    expect(attachmentThumbnailSrc(notes)).toBeNull();
  });
});
