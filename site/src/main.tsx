import { hydrateRoot } from "react-dom/client"
import Root from "./Root"
import "./styles.css"

hydrateRoot(document.getElementById("root")!, <Root />)
