export const usePathParams = (path: string) => {
  const pathParams = path.match(/:(\w+)/g)
  return pathParams?.map((param) => param.slice(1)) ?? []
}
