import React from 'react';



function AmountInput(props) {

  let handleSubmit = (event) => {
    event.preventDefault();
    let amount = event.target.amount.value;
    props.action(amount);
  };

  return (
    <form onSubmit={handleSubmit}>
      <div className="field has-addons">
        <div className="control has-icons-left">
          <input name='amount' className="input" type="text" placeholder="Amount" />
          <span className="icon is-small is-left">
            <i className={`mdi ${props.icon}`}></i>
          </span>
        </div>
        <div className="control">
          <button className={`button ${props.button}`} disabled={props.disabled} type="submit">
             {props.label}
          </button>
        </div>
      </div>
    </form>
  );

}

export default AmountInput;
