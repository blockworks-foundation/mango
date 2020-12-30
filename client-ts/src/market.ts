export class Market {
  greeting: string;

  constructor(initGreet?: string) {
	this.greeting = initGreet ?? 'hello world';
  }

  greet() {
	console.log(this.greeting);
  }
}
