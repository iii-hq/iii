import type { HTMLAttributes, ReactNode } from "react"
import type { Stage } from "./viz-data"

// @types/react has no MathML intrinsics yet; React DOM itself renders them in the
// MathML namespace, so declaring the handful we use is enough.
type MathProps = HTMLAttributes<HTMLElement>
declare module "react" {
  namespace JSX {
    interface IntrinsicElements {
      math: MathProps
      mi: MathProps
      mo: MathProps
      mn: MathProps
      mrow: MathProps
      mfrac: MathProps
      mtext: MathProps
    }
  }
}

/** n(n−1), the pairwise-edge numerator shared by the first two stages */
function PairsNumerator() {
  return (
    <mrow>
      <mi>n</mi>
      <mo>(</mo>
      <mi>n</mi>
      <mo>{"−"}</mo>
      <mn>1</mn>
      <mo>)</mo>
    </mrow>
  )
}

function BigO({ children }: { children: ReactNode }) {
  return (
    <math>
      <mi>O</mi>
      <mo>(</mo>
      {children}
      <mo>)</mo>
    </math>
  )
}

/** O(n(n−1)/2) → O(n(n−1)/(2·tolerance)) → O(0) */
export function ComplexityFormula({ stage }: { stage: Stage }) {
  if (stage === "iii") {
    return (
      <BigO>
        <mn>0</mn>
      </BigO>
    )
  }
  return (
    <BigO>
      <mfrac>
        <PairsNumerator />
        {stage === "mesh" ? (
          <mn>2</mn>
        ) : (
          <mrow>
            <mn>2</mn>
            <mo>{"·"}</mo>
            <mtext>your tolerance for integrations</mtext>
          </mrow>
        )}
      </mfrac>
    </BigO>
  )
}
