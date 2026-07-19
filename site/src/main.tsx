import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { domAnimation, LazyMotion, MotionConfig } from "motion/react"
import App from "./App"
import "./styles.css"

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <MotionConfig reducedMotion="user">
      <LazyMotion features={domAnimation} strict>
        <App />
      </LazyMotion>
    </MotionConfig>
  </StrictMode>,
)
