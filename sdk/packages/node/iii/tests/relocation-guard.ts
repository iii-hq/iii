// Compile-time guard for the 0.18.0 relocation of type-only symbols out of the
// package root into the `iii-sdk/types` entry point. Type-only exports leave no
// runtime trace, so this boundary can only be enforced by the TypeScript
// compiler (run via `tsc --noEmit`), not by a runtime test.

// All relocated symbols must remain importable from the new `types` entry point.
import type {
  ApiRequest,
  ApiResponse,
  AuthInput,
  AuthResult,
  HttpRequest,
  HttpResponse,
  IIIConnectionState,
  MessageType,
} from '../src/public-types'

export type _Msg = MessageType
export type _State = IIIConnectionState
export type _Api = [ApiRequest, ApiResponse, HttpRequest, HttpResponse, AuthInput, AuthResult]

// These must NO LONGER be exported from the package root. If any still is, the
// matching `@ts-expect-error` becomes unused and `tsc` fails with TS2578.
// @ts-expect-error relocated to `iii-sdk/types` in 0.18.0
import type { MessageType as _RootMessageType } from '../src/index'
// @ts-expect-error relocated to `iii-sdk/types` in 0.18.0
import type { ApiRequest as _RootApiRequest } from '../src/index'
// @ts-expect-error relocated to `iii-sdk/types` in 0.18.0
import type { AuthInput as _RootAuthInput } from '../src/index'
// @ts-expect-error relocated to `iii-sdk/types` in 0.18.0
import type { HttpResponse as _RootHttpResponse } from '../src/index'
