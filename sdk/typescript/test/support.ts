import { fileURLToPath } from "node:url"

export const helper = fileURLToPath(new URL("./fixtures/fake-helper.mjs", import.meta.url))

export const options = () => ({ command: [process.execPath, helper] as const })

export const processIsAlive = (pid: number): boolean => {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}
