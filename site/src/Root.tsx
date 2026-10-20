import { StrictMode } from "react"
import { domAnimation, LazyMotion, MotionConfig } from "motion/react"
import App from "./App"

export default function Root() {
  return (
    <StrictMode>
      <MotionConfig reducedMotion="user">
        <LazyMotion features={domAnimation} strict>
          <App />
        </LazyMotion>
      </MotionConfig>
    </StrictMode>
  )
}
