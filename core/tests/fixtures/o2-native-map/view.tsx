export type Props = { name: string; onSelect: (name: string) => void };
const example = 'const Card is documentation, not a declaration';
export const Card = (props: Props) => (
  <button onClick={() => props.onSelect(props.name)}>{props.name}</button>
);
// View renders the Card, without declaring another Card.
export function View(props: Props) {
  return <section><Card {...props} /></section>;
}
