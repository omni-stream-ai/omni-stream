import type { Id, Project, Script, Task, TestProfile } from '@omnistream/protocol';

export interface Store { projects: Map<Id, Project>; tasks: Map<Id, Task>; scripts: Map<Id, Script>; tests: Map<Id, TestProfile>; }
export class MemoryStore implements Store {
  projects = new Map<Id, Project>(); tasks = new Map<Id, Task>(); scripts = new Map<Id, Script>(); tests = new Map<Id, TestProfile>();
}
// The repository boundary is intentionally stable so a better-sqlite3 or libSQL implementation can replace this store.
export function createStore(): Store { return new MemoryStore(); }
