export const formatArgumentValue = (arg: string | object | false | null): string => {
  if (typeof arg === 'object' && arg !== null) {
    return JSON.stringify(arg, null, 1)
  }
  if (arg === null) {
    return 'null'
  }

  return arg.toString()
}
