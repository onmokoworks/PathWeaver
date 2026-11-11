// PathWeaver patch: an ancestor can provide how the number fields below it read
// and show values (e.g. a measurement unit), without each field — or the
// InputVec / InputPosition / InputSize wrappers in between — taking new props.
import type {InjectionKey, Ref} from 'vue'

export interface InputNumberContext {
	/** Rewrites typed text before it is evaluated (e.g. `10mm` → `(10*2.83)`). */
	expression?: (src: string) => string
	/** Shown after the value when the field has no suffix of its own. */
	suffix?: string
	/** Replaces the `precision` prop (max decimals shown). */
	precision?: number
	/** Unranged, unstepped drag: value change per pixel (default 1). */
	speed?: number
	/** Unstepped arrow keys: value change per press (default 1). */
	keyStep?: number
}

export const InputNumberContextKey: InjectionKey<Readonly<Ref<InputNumberContext>>> =
	Symbol('TqInputNumberContext')
