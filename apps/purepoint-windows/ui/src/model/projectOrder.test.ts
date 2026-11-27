import { describe, expect, it } from 'vitest';
import { movingProject, ProjectOrderSaver, sortedByProjectOrder } from './projectOrder.ts';

// The sidebar follows the order each machine's daemon saves, and moves send it back.
// (Port of ProjectOrderTests.swift.)
describe('ProjectOrderTests', () => {
  const moved = (root: string, index: number, roots: readonly string[]) =>
    movingProject(root, index, roots, (r) => r);

  it('givenAMoveShouldPlaceTheProjectAtTheIndex', () => {
    expect(moved('/c', 0, ['/a', '/b', '/c'])).toEqual(['/c', '/a', '/b']);
    expect(moved('/a', 1, ['/a', '/b', '/c'])).toEqual(['/b', '/a', '/c']);
    expect(moved('/a', 99, ['/a', '/b', '/c'])).toEqual(['/b', '/c', '/a']);
  });

  it('givenAMoveThatChangesNothingShouldReturnNil', () => {
    expect(moved('/a', 0, ['/a', '/b'])).toBeNull();
    expect(moved('/a', -1, ['/a', '/b'])).toBeNull();
    expect(moved('/missing', 0, ['/a', '/b'])).toBeNull();
  });

  it('givenTheDaemonOrderShouldSortListedProjectsFirstAndKeepTheRest', () => {
    const sorted = sortedByProjectOrder(['/a', '/x', '/b', '/y'], ['/b', '/gone', '/a'], (r) => r);
    expect(sorted).toEqual(['/b', '/a', '/x', '/y']);
  });
});

// Not in the Swift suite: the saver's serial sends, which Swift leaves to the daemon tests.
describe('ProjectOrderSaver', () => {
  it('givenTwoMovesShouldSendThemInOrderAndReportOnlyTheFailedOne', async () => {
    const sent: string[][] = [];
    const saver = new ProjectOrderSaver((roots) => {
      sent.push([...roots]);
      return Promise.resolve(sent.length === 1);
    });
    let failures = 0;

    const first = saver.save(['/b', '/a'], () => (failures += 1));
    const second = saver.save(['/a', '/b'], () => (failures += 1));
    expect(saver.pending).toBe(2);
    await Promise.all([first, second]);

    expect(sent).toEqual([
      ['/b', '/a'],
      ['/a', '/b'],
    ]);
    expect(saver.pending).toBe(0);
    expect(failures).toBe(1);
  });
});
