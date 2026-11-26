// The Point Guard spec's Credential Manager hygiene (WIN-9.5): the chat saves the managed bridge's
// pairing secret as a generic credential named `<endpoint>.PurePoint.PointGuard.Pairing`
// (REQ-WINAPP-075). The spec lists the loopback ones before it runs and deletes only those it
// added, so nothing it leaves behind outlives the run and nothing of the user's is touched.
import { spawnSync } from 'node:child_process';

const SERVICE = '.PurePoint.PointGuard.Pairing';

/** The loopback Point Guard pairing credentials, by the target `cmdkey /list` prints. */
export function pairingCredentials(): Set<string> {
  const result = spawnSync('cmdkey.exe', ['/list'], { encoding: 'utf8', windowsHide: true });
  const targets = new Set<string>();
  for (const line of result.stdout.split(/\r?\n/)) {
    const match = /^\s*Target:\s*(.+?)\s*$/.exec(line);
    const target = match?.[1];
    if (target === undefined) continue;
    if (target.includes('ws://127.0.0.1:') && target.endsWith(SERVICE)) targets.add(target);
  }
  return targets;
}

/** Delete the loopback pairing credentials that were not there `before`. */
export function removePairingCredentials(before: ReadonlySet<string>): void {
  for (const target of pairingCredentials()) {
    if (before.has(target)) continue;
    const bare = target.replace(/^LegacyGeneric:target=/, '');
    for (const name of [target, bare]) {
      const result = spawnSync('cmdkey.exe', [`/delete:${name}`], { windowsHide: true });
      if (result.status === 0) break;
    }
  }
}
