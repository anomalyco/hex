import { renderToString } from "react-dom/server"
import Root from "./Root"

export function render(): string {
  return renderToString(<Root />)
}
