import { format } from './util';

export class Client {
  greet(name: string): string {
    return format(`hello ${name}`);
  }
}
