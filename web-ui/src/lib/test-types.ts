// 类型桩：`describe` / `it` / `expect` 兼容 node:test 与 vitest 的 API 形态。
//
// 当前 `tsc --noEmit` 用这套类型替身检查 .test.ts 编译通过；实际运行需要
// 选定 runner（node:test + tsx 或 vitest），把这里的 import 替换为真模块即可。
// 详见 tasks/todo.md P2-10。

export type TestFn = () => void | Promise<void>

export interface Describe {
  (name: string, fn: () => void): void
}

export interface It {
  (name: string, fn: TestFn): void
}

export interface Expect {
  <T>(actual: T): {
    toEqual(expected: T): void
    toBe(expected: unknown): void
    toBeNull(): void
    toBeUndefined(): void
    toBeTruthy(): void
    toBeFalsy(): void
    toContain(item: unknown): void
    toThrow(): void
  }
}

export const describe: Describe = () => {}
export const it: It = () => {}
export const expect: Expect = <T>(_actual: T) => ({
  toEqual: () => undefined,
  toBe: () => undefined,
  toBeNull: () => undefined,
  toBeUndefined: () => undefined,
  toBeTruthy: () => undefined,
  toBeFalsy: () => undefined,
  toContain: () => undefined,
  toThrow: () => undefined,
})
