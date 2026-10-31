/**
 * The observer filter language (`iii-filter` v1, filters.md): validation and
 * evaluation.
 *
 * This is the Node SDK's evaluator (`sdk/packages/node/iii/src/filter`), which
 * is pure TypeScript with no Node.js APIs. It is imported, not copied, so the
 * two SDKs cannot drift: the build bundles it into `dist` (the published
 * package does not depend on `iii-sdk`), and this package's tests run the
 * shared conformance vectors against it.
 */
export {
  type CompiledFilter,
  compileFilter,
  type FilterDoc,
  FilterError,
  type FilterErrorCode,
  type FilterSource,
  MATCH_ALL,
} from '../../iii/src/filter'
