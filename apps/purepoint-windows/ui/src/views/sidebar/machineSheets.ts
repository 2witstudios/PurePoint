// Which machine sheet is up (WIN-8.14): Add Machine (Swift `SidebarFooter.showAddMachine`) and
// the remote folder picker a machine row's add button opens.
import { create } from 'zustand';

interface MachineSheetsState {
  readonly addMachine: boolean;
  /** The machine whose folders the picker browses; null while it is closed. */
  readonly folderPickerMachineId: string | null;
  showAddMachine(show: boolean): void;
  openFolderPicker(machineId: string | null): void;
}

export const useMachineSheets = create<MachineSheetsState>()((set) => ({
  addMachine: false,
  folderPickerMachineId: null,
  showAddMachine: (show) => {
    set({ addMachine: show });
  },
  openFolderPicker: (machineId) => {
    set({ folderPickerMachineId: machineId });
  },
}));
