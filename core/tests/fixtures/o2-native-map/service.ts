export interface User { id: number }
export type Result<T> = T | undefined;
export class Repository<T extends User> {
  constructor(private records: T[]) {}
  lookup(id: number): Result<T> {
    return this.records.find(record => record.id === id);
  }
  refresh(): void { this.lookup(1); }
}
export const sync = async (): Promise<boolean> => true;
export namespace Internal { export function active(): boolean { return true; } }
const example = 'function lookup is documentation, not a declaration';
