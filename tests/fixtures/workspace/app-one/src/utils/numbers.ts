// copy of lxp-common-widgets-js formatAmountWithCommas under a local name —
// mirrors the real numberWithCommas/formatNumberWithCommas case
export const numberWithCommas = (value?: number | string) => {
  if (value === 0) return '0'
  if (!value) return ''
  return value.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',')
}
