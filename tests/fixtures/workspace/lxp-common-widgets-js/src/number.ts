// canonical implementation living in a CLONED lxp-common-* repo — app copies
// of this body should be told to import it, not to create a new shared home
export const formatAmountWithCommas = (x: number) => {
  if (x === 0) return '0'
  if (!x) return ''
  return x.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',')
}
