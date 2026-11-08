import type {Decision,DecisionSnapshot} from '../api/decisions';
export function DecisionDeliveryStatus({current,snapshot}:{current:Decision;snapshot:DecisionSnapshot}) {
  const rows=snapshot.delivery?.filter(r=>r.decisions.some(d=>d.id===current.id && d.version===current.version))??[];
  return <details><summary>Delivery, acknowledgement and independent compliance</summary>
    {current.resolution && <p>Response recorded. Independent verification remains a separate check.</p>}
    {rows.length===0?<p>Delivery pending · acknowledgement pending · independent check pending for this response version.</p>:
      <div className="overflow-auto"><table className="w-full text-left text-xs"><thead><tr><th>Task / phase</th><th>Current context</th><th>Delivered</th><th>ACK</th><th>Review evidence</th></tr></thead>
        <tbody>{rows.map(r=><tr key={r.task+':'+r.phase}><td>{r.task} / {r.phase}</td><td>{r.current?'Current':'Refresh required'}</td><td>{r.delivered?'Yes':'Pending'}</td><td>{r.acknowledged?'Received':'Pending'}</td><td>{r.compliance?.[current.id]?.status??'Not independently checked'} {r.compliance?.[current.id]?.evidence}</td></tr>)}</tbody>
      </table></div>}
  </details>;
}
